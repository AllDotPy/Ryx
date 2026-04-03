"""
Ryx ORM — Migration Autodetector

Compares the current applied migration state (stored in the DB or in
migration files on disk) to the current model declarations, then generates
a new migration file with the needed changes.

This is the engine behind `python -m ryx makemigrations`.

Migration file format (plain Python):
  migrations/0001_initial.py
  migrations/0002_add_views_to_posts.py
  ...

Each file contains a `Migration` class with:
  - `dependencies`: list of migration names this one depends on
  - `operations`:   list of Operation objects (CreateTable, AddField, ...)

Operations:
  CreateTable(name, fields)
  AddField(model, name, field_deconstruct_dict)
  RemoveField(model, name)          # destructive — not auto-generated
  AlterField(model, name, field)
  CreateIndex(model, index)
  DeleteIndex(model, index_name)
  RunSQL(sql, reverse_sql)          # for raw migrations

Usage:
  detector = Autodetector(models=[Post, Author], migrations_dir="migrations/")
  changes = detector.detect()
  if changes:
      path = detector.write_migration(changes)
      print(f"Created {path}")
"""

from __future__ import annotations

import importlib
import importlib.util
import re
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Any, List, Optional

from ryx.migrations.state import (
    ColumnState, SchemaState, TableState,
    diff_states, project_state_from_models,
    ChangeKind, SchemaChange,
)


###
##      OPERATION CLASSES — serialisable migration actions
####
@dataclass
class CreateTable:
    """Create a new database table."""
    table: str
    columns: List[ColumnState]

    def describe(self) -> str:
        return f"Create table '{self.table}'"

    def to_python(self) -> str:
        cols = ", ".join(
            f'ColumnState(name={c.name!r}, db_type={c.db_type!r}, '
            f'nullable={c.nullable!r}, primary_key={c.primary_key!r}, '
            f'unique={c.unique!r})'
            for c in self.columns
        )
        return f"    CreateTable(table={self.table!r}, columns=[{cols}]),"


###
##      ADD FIELD
####
@dataclass
class AddField:
    """Add a column to an existing table."""
    table: str
    column: ColumnState

    def describe(self) -> str:
        return f"Add field '{self.column.name}' to '{self.table}'"

    def to_python(self) -> str:
        c = self.column
        return (
            f"    AddField(table={self.table!r}, "
            f"column=ColumnState(name={c.name!r}, db_type={c.db_type!r}, "
            f"nullable={c.nullable!r}, primary_key={c.primary_key!r}, "
            f"unique={c.unique!r})),"
        )


###
##      ALTTER FIELD
####
@dataclass
class AlterField:
    """Change a column's type or constraints."""
    table: str
    old_col: ColumnState
    new_col: ColumnState

    def describe(self) -> str:
        return (
            f"Alter field '{self.old_col.name}' on '{self.table}': "
            f"{self.old_col.db_type} → {self.new_col.db_type}"
        )

    def to_python(self) -> str:
        nc = self.new_col
        return (
            f"    AlterField(table={self.table!r}, "
            f"new_col=ColumnState(name={nc.name!r}, db_type={nc.db_type!r}, "
            f"nullable={nc.nullable!r})),"
        )


###
##      CREATE INDEX
####
@dataclass
class CreateIndex:
    """Create a database index."""
    table: str
    name: str
    fields: List[str]
    unique: bool = False

    def describe(self) -> str:
        return f"Create {'unique ' if self.unique else ''}index '{self.name}' on '{self.table}'"

    def to_python(self) -> str:
        return (
            f"    CreateIndex(table={self.table!r}, name={self.name!r}, "
            f"fields={self.fields!r}, unique={self.unique!r}),"
        )


###
##      RUN RAW SQL
####
@dataclass
class RunSQL:
    """Execute raw SQL (for manual migrations)."""
    sql:         str
    reverse_sql: str = ""

    def describe(self) -> str:
        return f"Run SQL: {self.sql[:60]}..."

    def to_python(self) -> str:
        return f"    RunSQL(sql={self.sql!r}, reverse_sql={self.reverse_sql!r}),"


# All operation types for isinstance checks
Operation = (CreateTable, AddField, AlterField, CreateIndex, RunSQL)


###
##      MIGRATION FILE MODEL
####
@dataclass
class MigrationFile:
    """Represents a single migration file."""
    name: str                 # e.g. "0001_initial"
    dependencies: List[str]           # migration names this depends on
    operations: List[Any]           # Operation instances


###
##      AUTODETECTOR
####
class Autodetector:
    """Detect schema changes and generate migration files.

    Args:
        models:         List of Model subclasses to inspect.
        migrations_dir: Path to the migrations directory (relative or absolute).
                        Created if it doesn't exist.
        app_label:      Optional app namespace prefix for migration names.
    """

    def __init__(
        self,
        models: List[type],
        migrations_dir: str = "migrations",
        app_label: str = "",
    ) -> None:
        self._models = models
        self._migrations_dir = Path(migrations_dir)
        self._app_label = app_label

    # Public API
    def detect(self) -> List[Any]:
        """Compare model declarations to the last applied migration state.

        Reads the most recent migration in the migrations directory to build
        the "current" state, then diffs it against the live model declarations.

        Returns:
            List of Operation objects representing needed changes.
        """
        current_state = self._load_applied_state()
        target_state  = project_state_from_models(self._models)
        changes = diff_states(current_state, target_state)
        return self._changes_to_operations(changes, target_state)

    def write_migration(self, operations: List[Any]) -> Path:
        """Write a migration file for the given operations.

        Creates the migrations directory if it doesn't exist.
        Auto-numbers the new migration based on existing files.

        Args:
            operations: List of Operation objects (from detect()).

        Returns:
            Path to the created migration file.
        """
        self._migrations_dir.mkdir(parents=True, exist_ok=True)
        self._ensure_init(self._migrations_dir)

        number = self._next_number()
        name_slug = self._make_slug(operations)
        prefix = f"{self._app_label}_" if self._app_label else ""
        file_name = f"{number:04d}_{prefix}{name_slug}.py"
        file_path = self._migrations_dir / file_name

        deps = self._last_migration_name()
        dep_list = f'["{deps}"]' if deps else "[]"

        ops_code = "\n".join(op.to_python() for op in operations)
        timestamp = datetime.utcnow().strftime("%Y-%m-%d %H:%M:%S")

        content = f'''# Auto-generated by ryx ORM — {timestamp}
# Do not edit manually unless you know what you are doing.

from ryx.migrations.autodetect import (
    CreateTable, AddField, AlterField, CreateIndex, RunSQL,
)
from ryx.migrations.state import ColumnState


class Migration:
    """Migration {file_name}

    Operations:
{chr(10).join("        " + op.describe() for op in operations)}
    """

    dependencies = {dep_list}

    operations = [
{ops_code}
    ]
'''
        file_path.write_text(content)
        return file_path

    # Internal helpers 
    def _load_applied_state(self) -> SchemaState:
        """Build the current state by replaying all applied migrations in order.

        If no migrations directory or no migration files exist, returns an
        empty SchemaState (fresh database).
        """
        if not self._migrations_dir.exists():
            return SchemaState()

        migration_files = sorted(self._migrations_dir.glob("[0-9]*.py"))
        if not migration_files:
            return SchemaState()

        state = SchemaState()

        for mf in migration_files:
            try:
                migration = self._load_migration_file(mf)
                self._apply_migration_to_state(migration, state)
            except Exception as e:
                import warnings
                warnings.warn(
                    f"Could not load migration {mf.name}: {e}",
                    stacklevel=2,
                )

        return state

    def _load_migration_file(self, path: Path) -> MigrationFile:
        """Import and return the Migration class from a migration file."""
        spec = importlib.util.spec_from_file_location(path.stem, path)
        mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)
        cls = mod.Migration
        return MigrationFile(
            name = path.stem,
            dependencies = cls.dependencies,
            operations = cls.operations,
        )

    def _apply_migration_to_state(self, mf: MigrationFile, state: SchemaState) -> None:
        """Apply the operations in a MigrationFile to a SchemaState."""
        for op in mf.operations:
            if isinstance(op, CreateTable):
                table = TableState(name=op.table)
                for col in op.columns:
                    table.add_column(col)
                state.add_table(table)

            elif isinstance(op, AddField):
                if state.has_table(op.table):
                    state.tables[op.table].add_column(op.column)

            elif isinstance(op, AlterField):
                if state.has_table(op.table) and state.tables[op.table].has_column(op.new_col.name):
                    state.tables[op.table].columns[op.new_col.name] = op.new_col

    def _changes_to_operations(
        self,
        changes: List[SchemaChange],
        target: SchemaState,
    ) -> List[Any]:
        """Convert SchemaChange diffs to Operation objects."""
        ops: List[Any] = []

        for change in changes:
            if change.kind == ChangeKind.CREATE_TABLE:
                table = target.tables.get(change.table)
                if table:
                    ops.append(CreateTable(
                        table = change.table,
                        columns = list(table.columns.values()),
                    ))

            elif change.kind == ChangeKind.ADD_COLUMN:
                if change.new_state:
                    ops.append(AddField(table=change.table, column=change.new_state))

            elif change.kind == ChangeKind.ALTER_COLUMN:
                if change.old_state and change.new_state:
                    ops.append(AlterField(
                        table = change.table,
                        old_col = change.old_state,
                        new_col = change.new_state,
                    ))

        # Also add index creation operations for all models
        for model in self._models:
            if not hasattr(model, "_meta"):
                continue
            meta  = model._meta
            table = meta.table_name

            for idx in meta.indexes:
                ops.append(CreateIndex(
                    table = table,
                    name = idx.name,
                    fields = idx.fields,
                    unique = idx.unique,
                ))

            for i, fields in enumerate(meta.index_together):
                name = f"idx_{table}_{'_'.join(fields)}_{i}"
                ops.append(CreateIndex(table=table, name=name, fields=list(fields)))

            for i, fields in enumerate(meta.unique_together):
                name = f"uq_{table}_{'_'.join(fields)}_{i}"
                ops.append(CreateIndex(table=table, name=name, fields=list(fields), unique=True))

        return ops

    def _next_number(self) -> int:
        """Return the next migration sequence number."""
        existing = sorted(self._migrations_dir.glob("[0-9]*.py"))
        if not existing:
            return 1
        last = existing[-1].name
        m = re.match(r"^(\d+)", last)
        return int(m.group(1)) + 1 if m else 1

    def _last_migration_name(self) -> Optional[str]:
        """Return the stem of the most recent migration file, or None."""
        existing = sorted(self._migrations_dir.glob("[0-9]*.py"))
        return existing[-1].stem if existing else None

    def _make_slug(self, operations: List[Any]) -> str:
        """Generate a short human-readable slug from the operation list."""
        if not operations:
            return "auto"
        first = operations[0]
        if isinstance(first, CreateTable):
            return f"create_{first.table}"
        if isinstance(first, AddField):
            return f"add_{first.column.name}_to_{first.table}"
        if isinstance(first, AlterField):
            return f"alter_{first.new_col.name}_on_{first.table}"
        return "auto"

    @staticmethod
    def _ensure_init(directory: Path) -> None:
        """Create __init__.py in the migrations directory if missing."""
        init = directory / "__init__.py"
        if not init.exists():
            init.write_text("# ryx migrations package\n")
