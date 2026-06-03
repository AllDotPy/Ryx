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
from datetime import datetime
from pathlib import Path
from typing import List, Optional, Set

from ryx import ryx_core as _core
from ryx.migrations.autodetect import (
    AddField,
    AlterField,
    CreateIndex,
    CreateTable,
    RunSQL,
    load_migration_file,
)
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

    Now supports multi-database routing.

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
        self,
        models: list,
        *,
        dry_run: bool = False,
        backend: Optional[str] = None,
        alias_filter: Optional[str] = None,
        migrations_dir: str = "migrations",
        no_interactive: bool = False,
    ) -> None:
        self._models = models
        self._dry_run = dry_run
        self._alias_filter = alias_filter
        self._migrations_dir = Path(migrations_dir)
        self._no_interactive = no_interactive
        # 'backend' is now a fallback if we can't detect it from the pool
        self._fallback_backend = backend.lower() if backend else "postgres"
        self._ddl = None  # Will be initialized per-database during migration

    async def migrate(self) -> List[SchemaChange]:
        """Detect and apply all pending schema changes across configured databases.

        Migration strategy (in order of preference):
          1. File-based migrations — reads ``.py`` files from ``{migrations_dir}/{alias}/``,
             applies pending ones, tracks in ``ryx_migrations`` table.
          2. No files found — interactive prompt offering live-DDL, auto-generate, or manual.

        Returns:
            A list of all SchemaChange objects applied across databases.
        """
        from ryx.router import get_router

        router = get_router()

        all_applied_changes: List[SchemaChange] = []
        aliases = _core.list_aliases()

        for alias in aliases:
            if self._alias_filter and alias != self._alias_filter:
                continue

            logger.info("Running migrations for database: %s", alias)

            # Setup backend and DDL generator for this alias
            try:
                backend = _core.get_backend(alias)
                logger.info("Backend for alias '%s': %s", alias, backend)
            except Exception as e:
                logger.warning(
                    "Could not detect backend for alias %s: %s. Falling back to %s",
                    alias, e, self._fallback_backend,
                )
                backend = self._fallback_backend

            self._current_backend = backend
            self._ddl = DDLGenerator(backend)
            self._current_alias = alias

            # Determine models for this alias
            models_for_db = self._filter_models_for_db(alias, router)
            if not models_for_db:
                logger.debug("No models mapped to database %s, skipping.", alias)
                continue

            # Determine migration directory for this alias
            #   Priority: {migrations_dir}/{alias}/  (multi-DB)  →  {migrations_dir}/  (single-DB, backward compat)
            alias_specific = self._migrations_dir / alias
            if alias_specific.exists() and list(alias_specific.glob("[0-9]*.py")):
                alias_dir = alias_specific
                has_alias_subdir = True
            else:
                alias_dir = self._migrations_dir
                has_alias_subdir = False

            migration_files = sorted(alias_dir.glob("[0-9]*.py")) if alias_dir.exists() else []

            if migration_files:
                changes = await self._apply_file_migrations(alias, migration_files)
                all_applied_changes.extend(changes)
            elif models_for_db:
                await self._handle_no_migration_files(
                    alias, alias_dir, models_for_db, has_alias_subdir,
                )

            # Always apply indexes, constraints, M2M tables
            if not self._dry_run:
                await self._apply_meta_extras(alias)

        logger.info("Multi-DB migration complete.")
        return all_applied_changes

    def _filter_models_for_db(self, alias: str, router) -> list:
        """Return models whose route maps to the given database alias."""
        models = []
        for model in self._models:
            db = None
            if router:
                db = router.db_for_write(model)
            if not db:
                db = getattr(model._meta, "database", None)
            if db == alias or (db is None and alias == "default"):
                models.append(model)
        return models

    # ------------------------------------------------------------------
    #  FILE-BASED MIGRATIONS
    # ------------------------------------------------------------------
    async def _apply_file_migrations(
        self,
        alias: str,
        migration_files: list,
    ) -> list:
        """Apply pending migration files to *alias*, tracking in ``ryx_migrations``.

        Returns list of SchemaChange objects for reporting.
        """
        await self._ensure_migrations_table(alias)
        applied: Set[str] = await self._get_applied_migrations(alias)

        pending = [f for f in migration_files if f.stem not in applied]
        if not pending:
            logger.info("Database %s is up to date.", alias)
            return []

        changes: List[SchemaChange] = []
        for mf_path in pending:
            logger.info("Applying: %s to %s", mf_path.stem, alias)
            print(f"[ryx] Applying {mf_path.stem} to {alias} ...")

            if self._dry_run:
                print(f"  Would apply: {mf_path.stem}")
                changes.append(SchemaChange(
                    kind=ChangeKind.CREATE_TABLE,
                    table=mf_path.stem,
                    description=f"Migration {mf_path.stem}",
                ))
                continue

            migration = load_migration_file(mf_path)
            for op in migration.operations:
                sql = self._operation_to_ddl(op)
                if sql:
                    logger.debug("SQL: %s", sql.strip())
                    from ryx.executor_helpers import raw_execute
                    try:
                        await raw_execute(sql, alias=alias)
                    except Exception as e:
                        logger.error(
                            "DDL failed in %s: %s — %s", mf_path.stem, sql, e
                        )
                        raise

            await self._record_migration(alias, mf_path.stem)
            print(f"[ryx]  ✓ {mf_path.stem} applied")

        return changes

    def _operation_to_ddl(self, op) -> Optional[str]:
        """Convert a migration Operation to a DDL SQL string."""
        if isinstance(op, CreateTable):
            table = TableState(name=op.table)
            for col in op.columns:
                table.add_column(col)
            return self._ddl.create_table(table)

        if isinstance(op, AddField):
            return self._ddl.add_column(op.table, op.column)

        if isinstance(op, AlterField):
            return self._ddl.alter_column(op.table, op.new_col)

        if isinstance(op, CreateIndex):
            return self._ddl.create_index_from_fields(
                op.table, op.fields, op.name, unique=op.unique,
            )

        if isinstance(op, RunSQL):
            return op.sql

        return None

    # ------------------------------------------------------------------
    #  INTERACTIVE FALLBACK  (no migration files found)
    # ------------------------------------------------------------------
    async def _handle_no_migration_files(
        self, alias: str, alias_dir: Path, models: list, has_alias_subdir: bool = False,
    ) -> None:
        """Called when no migration files exist for *alias*.

        Offers the user an interactive choice, or errors if ``--no-interactive``.
        """
        if self._no_interactive:
            print(
                f"[ryx] No migration files for '{alias}' and --no-interactive is set.\n"
                f"  Run 'ryx makemigrations --models <module> --alias {alias}' first."
            )
            return

        print(
            f"\n[ryx] No migration files found for database '{alias}'.\n"
            f"  {len(models)} model(s) are not yet tracked."
        )
        print()
        print("  [L] Live DDL — apply changes directly (development only)")
        print("  [A] Auto-generate migration files, then migrate")
        print("  [M] Manual — run 'ryx makemigrations --alias <alias>' first")
        print("  [S] Skip this database for now")
        print()

        choice = input("[ryx] Choice (L/A/M/S) [S]: ").strip().upper() or "S"

        if choice == "L":
            logger.info("Applying live DDL for %s ...", alias)
            current_state = await self._introspect_schema(alias)
            target_state = project_state_from_models(models)
            changes = diff_states(current_state, target_state)
            if changes:
                print(f"[ryx] Applying {len(changes)} live change(s) to {alias}")
                await self._apply_changes(changes, target_state, alias)

        elif choice == "A":
            logger.info("Auto-generating migration files for %s ...", alias)
            from ryx.migrations.autodetect import Autodetector

            alias_arg = alias if has_alias_subdir or alias != "default" else None
            detector = Autodetector(
                models=models,
                migrations_dir=str(self._migrations_dir),
                alias=alias_arg,
            )
            operations = detector.detect()
            if not operations:
                print("[ryx] No changes detected.")
                return
            path = detector.write_migration(operations)
            print(f"[ryx] Created migration: {path}")

            migration = load_migration_file(path)
            await self._ensure_migrations_table(alias)
            for op in migration.operations:
                sql = self._operation_to_ddl(op)
                if sql:
                    from ryx.executor_helpers import raw_execute
                    await raw_execute(sql, alias=alias)
            await self._record_migration(alias, path.stem)
            print(f"[ryx]  ✓ {path.stem} applied")

        elif choice == "M":
            alias_flag = f" --alias {alias}" if alias != "default" else ""
            print(f"[ryx] Run: ryx makemigrations --models <module>{alias_flag}")
            print(f"[ryx] Then run 'ryx migrate' again.")

        else:
            print(f"[ryx] Skipping database '{alias}'.")

    # ------------------------------------------------------------------
    #  MIGRATION TRACKING TABLE
    # ------------------------------------------------------------------
    async def _get_applied_migrations(self, alias: str) -> Set[str]:
        """Return set of migration names already recorded in the tracking table."""
        applied: Set[str] = set()
        try:
            from ryx.executor_helpers import raw_fetch

            rows = await raw_fetch(
                f"SELECT name FROM {MIGRATIONS_TABLE} ORDER BY id",
                alias=alias,
            )
            for row in rows:
                applied.add(row.get("name", ""))
        except Exception:
            pass
        return applied

    async def _record_migration(self, alias: str, name: str) -> None:
        """Insert a row into the tracking table after a migration is applied."""
        from ryx.executor_helpers import raw_execute

        ts = datetime.utcnow().strftime("%Y-%m-%d %H:%M:%S")
        sql = (
            f"INSERT INTO {MIGRATIONS_TABLE} (name, applied_at) "
            f"VALUES ('{name}', '{ts}')"
        )
        try:
            await raw_execute(sql, alias=alias)
        except Exception as e:
            logger.warning("Could not record migration '%s': %s", name, e)

    # Schema introspection
    async def _introspect_schema(self, alias: str) -> SchemaState:
        """Query the live database to build a current SchemaState."""
        state = SchemaState()

        tables = await self._get_tables(alias)
        for table_name in tables:
            if not table_name or table_name.startswith("ryx_"):
                continue
            columns = await self._get_columns(table_name, alias)
            tbl = TableState(name=table_name)
            for col in columns:
                tbl.add_column(col)
            state.add_table(tbl)

        return state

    async def _get_tables(self, alias: str) -> List[str]:
        """Return the list of user table names from the live DB."""
        from ryx.executor_helpers import raw_fetch

        # information_schema (Postgres / MySQL)
        try:
            rows = await raw_fetch(
                "SELECT table_name FROM information_schema.tables "
                "WHERE table_schema = 'public' AND table_type = 'BASE TABLE'",
                alias=alias,
            )
            if rows:
                return [r.get("table_name", "") for r in rows]
        except Exception:
            pass

        # SQLite fallback
        try:
            rows = await raw_fetch(
                "SELECT name AS table_name FROM sqlite_master WHERE type='table'",
                alias=alias,
            )
            return [r.get("table_name", "") for r in rows]
        except Exception:
            return []

    async def _get_columns(self, table_name: str, alias: str) -> List[ColumnState]:
        """Return ColumnState objects for each column in the given table."""
        from ryx.executor_helpers import raw_fetch

        cols: List[ColumnState] = []

        # information_schema (Postgres / MySQL)
        try:
            rows = await raw_fetch(
                f"SELECT column_name, data_type, is_nullable, column_default "
                f"FROM information_schema.columns "
                f"WHERE table_name = '{table_name}' ORDER BY ordinal_position",
                alias=alias,
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
            rows = await raw_fetch(f'PRAGMA table_info("{table_name}")', alias=alias)
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
    def _print_dry_run(
        self, changes: List[SchemaChange], target: SchemaState, alias: str
    ) -> None:
        """Print the SQL that would be executed."""
        logger.info("[DRY RUN] SQL for database %s that would be executed:", alias)
        for ch in changes:
            sql = self._ddl_for_change(ch, target)
            if sql:
                logger.info("  %s;", sql)

    async def _apply_changes(
        self, changes: List[SchemaChange], target: SchemaState, alias: str
    ) -> None:
        """Execute DDL for each detected change."""
        from ryx.executor_helpers import raw_execute

        for ch in changes:
            sql = self._ddl_for_change(ch, target)
            if not sql:
                continue
            logger.info("[%s] Applying: %s", alias, ch)
            logger.debug("SQL: %s", sql)
            try:
                await raw_execute(sql, alias=alias)
            except Exception as e:
                logger.error("DDL failed on %s: %s — %s", alias, sql, e)
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
                    self._current_backend,
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

    async def _apply_meta_extras(self, alias: str) -> None:
        """Apply indexes, unique_together, and constraints from Meta classes.

        These are idempotent (IF NOT EXISTS) so safe to re-run on every migrate.
        """
        from ryx.executor_helpers import raw_execute

        for model in self._models:
            if not hasattr(model, "_meta"):
                continue
            meta = model._meta
            table = meta.table_name

            # Only apply if the model belongs to this database
            # (Basically duplicate the routing logic here or use a helper)
            from ryx.router import get_router

            router = get_router()
            db = None
            if router:
                db = router.db_for_write(model)
            if not db:
                db = getattr(meta, "database", None)

            if db != alias and (db is not None or alias != "default"):
                continue

            # Named indexes from Meta.indexes
            for idx in meta.indexes:
                sql = self._ddl.create_index(table, idx)
                logger.debug("Index DDL: %s", sql)
                try:
                    await raw_execute(sql, alias=alias)
                except Exception as e:
                    logger.debug("Index already exists or error: %s", e)

            # index_together
            for i, fields in enumerate(meta.index_together):
                name = f"idx_{table}_{'_'.join(fields)}_{i}"
                sql = self._ddl.create_index_from_fields(table, list(fields), name)
                try:
                    await raw_execute(sql, alias=alias)
                except Exception:
                    pass

            # unique_together
            for i, fields in enumerate(meta.unique_together):
                name = f"uq_{table}_{'_'.join(fields)}_{i}"
                sql = self._ddl.create_index_from_fields(
                    table, list(fields), name, unique=True
                )
                try:
                    await raw_execute(sql, alias=alias)
                except Exception:
                    pass

            # CHECK constraints (not supported by all backends)
            for constraint in meta.constraints:
                sql = self._ddl.add_constraint(table, constraint)
                if sql:
                    try:
                        await raw_execute(sql, alias=alias)
                    except Exception:
                        pass  # constraint may already exist

            # ManyToMany join tables
            for fname, m2m_field in meta.many_to_many.items():
                await self._ensure_m2m_table(m2m_field, alias)

    async def _ensure_m2m_table(self, m2m_field, alias: str) -> None:
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
            await raw_execute(sql, alias=alias)
            # Unique constraint on (source_fk, target_fk) to prevent duplicates
            uq_sql = self._ddl.create_index_from_fields(
                join_table,
                [source_fk, target_fk],
                f"uq_{join_table}_pair",
                unique=True,
            )
            await raw_execute(uq_sql, alias=alias)
        except Exception:
            pass  # join table already exists

    # Migrations tracking table
    async def _ensure_migrations_table(self, alias: str) -> None:
        """Create the Ryx migrations tracking table if it doesn't exist."""
        from ryx.executor_helpers import raw_execute

        tbl = TableState(name=MIGRATIONS_TABLE)
        tbl.add_column(ColumnState("id", "INTEGER", nullable=False, primary_key=True))
        tbl.add_column(ColumnState("name", "VARCHAR(255)", nullable=False, unique=True))
        tbl.add_column(ColumnState("applied_at", "TIMESTAMP", nullable=False))

        sql = self._ddl.create_table(tbl)
        try:
            await raw_execute(sql, alias=alias)
        except Exception:
            pass  # table already exists
