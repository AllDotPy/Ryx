"""
Ryx ORM — Migration Runner  (backend-aware, full DDL support)

Applies pending schema changes to the live database.
Uses DDLGenerator for backend-correct SQL (Postgres / MySQL / SQLite).

Steps:
  1. Ensure the ryx_migrations tracking table exists
  2. Introspect the live database schema
  3. Build the target schema from Model declarations
  4. Diff the two states
  5. Generate DDL via DDLGenerator (backend-aware)
  6. Execute each DDL statement
  7. Also create indexes and constraints declared in Model.Meta
"""

from __future__ import annotations

import logging
import os
from typing import List, Optional

from ryx import ryx_core as _core
from ryx.migrations.state import (
    ChangeKind,
    ColumnState,
    SchemaChange,
    SchemaState,
    TableState,
    diff_states,
    project_state_from_models,
)
from ryx.migrations.ddl import DDLGenerator, detect_backend

logger = logging.getLogger("ryx.migrations")
MIGRATIONS_TABLE = "ryx_migrations"


###
##      MIGRATION RUNNER
####
class MigrationRunner:
    """Apply pending schema changes to the live database.

    Usage::

        from ryx.migrations import MigrationRunner
        runner = MigrationRunner([Post, Author, Comment])
        await runner.migrate()

        # Preview only
        await runner.migrate(dry_run=True)

    Args:
        models:  List of Model subclasses whose schema should be applied.
        dry_run: If True, print SQL without executing. Default: False.
    """

    def __init__(
        self, models: list, *, dry_run: bool = False, backend: Optional[str] = None
    ) -> None:
        self._models = models
        self._dry_run = dry_run
        # Detect backend from explicit argument, env var, or default to postgres
        if backend:
            self._backend = backend.lower()
        else:
            url = os.environ.get("RYX_DATABASE_URL", "")
            self._backend = detect_backend(url) if url else "postgres"
        self._ddl = DDLGenerator(self._backend)

    async def migrate(self) -> List[SchemaChange]:
        """Detect and apply all pending schema changes.

        Returns:
            List of SchemaChange objects applied (or that would be applied
            in dry-run mode).
        """
        await self._ensure_migrations_table()

        current_state = await self._introspect_schema()
        target_state = project_state_from_models(self._models)
        changes = diff_states(current_state, target_state)

        if not changes:
            logger.info("No schema changes detected — database is up to date.")
        else:
            logger.info("Detected %d schema change(s):", len(changes))
            for ch in changes:
                logger.info("  - %s", ch)

        if self._dry_run:
            self._print_dry_run(changes, target_state)
            return changes

        await self._apply_changes(changes, target_state)
        await self._apply_meta_extras()

        logger.info("Migration complete.")
        return changes

    # Schema introspection
    async def _introspect_schema(self) -> SchemaState:
        """Query the live database to build a current SchemaState."""
        state = SchemaState()

        tables = await self._get_tables()
        for table_name in tables:
            if not table_name or table_name.startswith("ryx_"):
                continue
            columns = await self._get_columns(table_name)
            tbl = TableState(name=table_name)
            for col in columns:
                tbl.add_column(col)
            state.add_table(tbl)

        return state

    async def _get_tables(self) -> List[str]:
        """Return the list of user table names from the live DB."""
        from ryx.executor_helpers import raw_fetch

        # information_schema (Postgres / MySQL)
        try:
            rows = await raw_fetch(
                "SELECT table_name FROM information_schema.tables "
                "WHERE table_schema = 'public' AND table_type = 'BASE TABLE'"
            )
            if rows:
                return [r.get("table_name", "") for r in rows]
        except Exception:
            pass

        # SQLite fallback
        try:
            rows = await raw_fetch(
                "SELECT name AS table_name FROM sqlite_master WHERE type='table'"
            )
            return [r.get("table_name", "") for r in rows]
        except Exception:
            return []

    async def _get_columns(self, table_name: str) -> List[ColumnState]:
        """Return ColumnState objects for each column in the given table."""
        from ryx.executor_helpers import raw_fetch

        cols: List[ColumnState] = []

        # information_schema (Postgres / MySQL)
        try:
            rows = await raw_fetch(
                f"SELECT column_name, data_type, is_nullable, column_default "
                f"FROM information_schema.columns "
                f"WHERE table_name = '{table_name}' ORDER BY ordinal_position"
            )
            if rows:
                for row in rows:
                    cols.append(
                        ColumnState(
                            name=row.get("column_name", "?"),
                            db_type=(row.get("data_type") or "TEXT").upper(),
                            nullable=row.get("is_nullable", "YES") == "YES",
                            default=row.get("column_default"),
                        )
                    )
                return cols
        except Exception:
            pass

        # SQLite PRAGMA
        try:
            rows = await raw_fetch(f'PRAGMA table_info("{table_name}")')
            for row in rows:
                cols.append(
                    ColumnState(
                        name=row.get("name", "?"),
                        db_type=(row.get("type") or "TEXT").upper(),
                        nullable=not bool(row.get("notnull", 0)),
                        primary_key=bool(row.get("pk", 0)),
                        default=row.get("dflt_value"),
                    )
                )
        except Exception:
            pass

        return cols

    # DDL execution
    def _print_dry_run(self, changes: List[SchemaChange], target: SchemaState) -> None:
        """Print the SQL that would be executed."""
        logger.info("[DRY RUN] SQL that would be executed:")
        for ch in changes:
            sql = self._ddl_for_change(ch, target)
            if sql:
                logger.info("  %s;", sql)

    async def _apply_changes(
        self, changes: List[SchemaChange], target: SchemaState
    ) -> None:
        """Execute DDL for each detected change."""
        from ryx.executor_helpers import raw_execute

        for ch in changes:
            sql = self._ddl_for_change(ch, target)
            if not sql:
                continue
            logger.info("Applying: %s", ch)
            logger.debug("SQL: %s", sql)
            try:
                await raw_execute(sql)
            except Exception as e:
                logger.error("DDL failed: %s — %s", sql, e)
                raise

    def _ddl_for_change(
        self, change: SchemaChange, target: SchemaState
    ) -> Optional[str]:
        """Generate DDL SQL for a single SchemaChange."""

        if change.kind == ChangeKind.CREATE_TABLE:
            table = target.tables.get(change.table)
            if table:
                return self._ddl.create_table(table)

        elif change.kind == ChangeKind.ADD_COLUMN and change.new_state:
            return self._ddl.add_column(change.table, change.new_state)

        elif change.kind == ChangeKind.ALTER_COLUMN and change.new_state:
            sql = self._ddl.alter_column(change.table, change.new_state)
            if sql is None:
                logger.warning(
                    "ALTER COLUMN not supported on %s for %s.%s — "
                    "manual migration required.",
                    self._backend,
                    change.table,
                    change.column,
                )
            return sql

        else:
            # DROP_TABLE / DROP_COLUMN — intentionally not auto-generated.
            logger.warning(
                "Skipping %s on '%s' — destructive operations require "
                "manual migration files.",
                change.kind.name,
                change.table,
            )

        return None

    async def _apply_meta_extras(self) -> None:
        """Apply indexes, unique_together, and constraints from Meta classes.

        These are idempotent (IF NOT EXISTS) so safe to re-run on every migrate.
        """
        from ryx.executor_helpers import raw_execute

        for model in self._models:
            if not hasattr(model, "_meta"):
                continue
            meta = model._meta
            table = meta.table_name

            # Named indexes from Meta.indexes
            for idx in meta.indexes:
                sql = self._ddl.create_index(table, idx)
                logger.debug("Index DDL: %s", sql)
                try:
                    await raw_execute(sql)
                except Exception as e:
                    logger.debug("Index already exists or error: %s", e)

            # index_together
            for i, fields in enumerate(meta.index_together):
                name = f"idx_{table}_{'_'.join(fields)}_{i}"
                sql = self._ddl.create_index_from_fields(table, list(fields), name)
                try:
                    await raw_execute(sql)
                except Exception:
                    pass

            # unique_together
            for i, fields in enumerate(meta.unique_together):
                name = f"uq_{table}_{'_'.join(fields)}_{i}"
                sql = self._ddl.create_index_from_fields(
                    table, list(fields), name, unique=True
                )
                try:
                    await raw_execute(sql)
                except Exception:
                    pass

            # CHECK constraints (not supported by all backends)
            for constraint in meta.constraints:
                sql = self._ddl.add_constraint(table, constraint)
                if sql:
                    try:
                        await raw_execute(sql)
                    except Exception:
                        pass  # constraint may already exist

            # ManyToMany join tables
            for fname, m2m_field in meta.many_to_many.items():
                await self._ensure_m2m_table(m2m_field)

    async def _ensure_m2m_table(self, m2m_field) -> None:
        """Create the join table for a ManyToManyField if it doesn't exist."""
        from ryx.executor_helpers import raw_execute
        from ryx.migrations.state import TableState, ColumnState

        join_table = getattr(m2m_field, "_join_table", None)
        source_fk = getattr(m2m_field, "_source_fk", None)
        target_fk = getattr(m2m_field, "_target_fk", None)

        if not all([join_table, source_fk, target_fk]):
            return

        # Build a TableState for the join table
        tbl = TableState(name=join_table)
        tbl.add_column(ColumnState("id", "INTEGER", nullable=False, primary_key=True))
        tbl.add_column(ColumnState(source_fk, "INTEGER", nullable=False))
        tbl.add_column(ColumnState(target_fk, "INTEGER", nullable=False))
        sql = self._ddl.create_table(tbl)

        try:
            await raw_execute(sql)
            # Unique constraint on (source_fk, target_fk) to prevent duplicates
            uq_sql = self._ddl.create_index_from_fields(
                join_table,
                [source_fk, target_fk],
                f"uq_{join_table}_pair",
                unique=True,
            )
            await raw_execute(uq_sql)
        except Exception:
            pass  # join table already exists

    # Migrations tracking table
    async def _ensure_migrations_table(self) -> None:
        """Create the Ryx migrations tracking table if it doesn't exist."""
        from ryx.executor_helpers import raw_execute

        tbl = TableState(name=MIGRATIONS_TABLE)
        tbl.add_column(ColumnState("id", "INTEGER", nullable=False, primary_key=True))
        tbl.add_column(ColumnState("name", "VARCHAR(255)", nullable=False, unique=True))
        tbl.add_column(ColumnState("applied_at", "TIMESTAMP", nullable=False))

        sql = self._ddl.create_table(tbl)
        try:
            await raw_execute(sql)
        except Exception:
            pass  # table already exists
