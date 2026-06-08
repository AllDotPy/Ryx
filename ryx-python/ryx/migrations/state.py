"""
Ryx ORM — Migration State

The migration system works by comparing two "states":
  1. The CURRENT state: what the database actually looks like right now
     (discovered by introspecting the DB schema at runtime).
  2. The PROJECT state: what the models say the schema should look like
     (derived from the Model class declarations in Python code).

The diff between these two states produces a list of SchemaChange objects,
which the MigrationRunner then executes as SQL DDL statements.

This file defines:
  - ColumnState:  a snapshot of a single column's definition
  - TableState:   a snapshot of all columns in a table
  - SchemaState:  a snapshot of the entire database schema (all tables)
  - SchemaChange: a single DDL operation (create table, add column, etc.)

Design note:
  We keep state objects as plain dataclasses (no DB logic here). This makes
  them easy to serialize to JSON for storing applied-migration history, and
  easy to compare in unit tests without a live database.
"""

from __future__ import annotations

import json
from dataclasses import dataclass, field
from enum import Enum, auto
from typing import Dict, List, Optional


###
##      COLUMNS SNAPSHOT
####
@dataclass
class ColumnState:
    """A snapshot of a single database column's definition.

    Attributes:
        name:         The column name in the database.
        db_type:      The SQL type string (e.g., ``"VARCHAR(200)"``).
        nullable:     Whether the column allows NULL values.
        primary_key:  Whether this column is (part of) the primary key.
        unique:       Whether a UNIQUE constraint exists on this column.
        default:      The SQL-level default expression, or None.
    """
    name: str
    db_type: str
    nullable: bool = True
    primary_key: bool = False
    unique: bool = False
    default: Optional[str] = None
    __table_state: Optional[TableState] = field(default=None, repr=False, compare=False)

    @property
    def table(self) -> Optional[TableState]:
        """Return the parent TableState this column belongs to, if set."""
        return self.__table_state

    def set_table_state(self, table_state: TableState) -> None:
        """Link this column state to its parent table state for context."""
        self.__table_state = table_state

    def __eq__(self, other: object) -> bool:
        """Two column states are equal if their definition is identical."""
        if not isinstance(other, ColumnState):
            return NotImplemented
        return (
            self.name == other.name
            and self.db_type.upper() == other.db_type.upper()
            and self.nullable == other.nullable
            and self.primary_key == other.primary_key
            and self.unique == other.unique
        )


###
##      TABLES SNAPSHOT
####
@dataclass
class TableState:
    """A snapshot of a single table's schema.

    Attributes:
        name:    The table name.
        schema:  Database schema (PostgreSQL). Empty string = default schema.
        columns: Ordered dict of column_name → ColumnState.
    """
    name: str
    schema: str = ""
    columns: Dict[str, ColumnState] = field(default_factory=dict)

    def add_column(self, col: ColumnState) -> None:
        """Register a column in this table's snapshot."""
        self.columns[col.name] = col
        col.set_table_state(self)

    def has_column(self, name: str) -> bool:
        """Return True if this table has a column with the given name."""
        return name in self.columns


###
##      SCHEMA SNAPSHOT
####
@dataclass
class SchemaState:
    """A snapshot of the entire database schema.

    Attributes:
        tables: Dict of table_name → TableState.
    """
    tables: Dict[str, TableState] = field(default_factory=dict)

    def add_table(self, table: TableState) -> None:
        """Register a table in the schema snapshot."""
        self.tables[table.name] = table

    def has_table(self, name: str) -> bool:
        """Return True if this schema contains a table with the given name."""
        return name in self.tables

    def to_json(self) -> str:
        """Serialize the schema state to a JSON string.

        Used by the migration runner to persist the applied-migration state
        in the ``Ryx_migrations`` tracking table.
        """
        data = {
            table_name: {
                "_schema": table.schema,
                "columns": {
                    col_name: {
                        "db_type": col.db_type,
                        "nullable": col.nullable,
                        "primary_key": col.primary_key,
                        "unique": col.unique,
                        "default": col.default,
                    }
                    for col_name, col in table.columns.items()
                },
            }
            for table_name, table in self.tables.items()
        }
        return json.dumps(data, indent=2)

    @classmethod
    def from_json(cls, raw: str) -> "SchemaState":
        """Deserialize a SchemaState from a JSON string."""
        state = cls()
        data = json.loads(raw)
        for table_name, table_data in data.items():
            columns_data = table_data.get("columns", table_data)
            table = TableState(name=table_name, schema=table_data.get("_schema", ""))
            for col_name, col_data in columns_data.items():
                table.add_column(ColumnState(
                    name = col_name,
                    db_type = col_data["db_type"],
                    nullable = col_data["nullable"],
                    primary_key = col_data["primary_key"],
                    unique = col_data["unique"],
                    default = col_data.get("default"),
                ))
            state.add_table(table)
        return state


###
##      SCHEMA KIND — the output of the diff
####
class ChangeKind(Enum):
    """The type of DDL change represented by a SchemaChange."""
    CREATE_TABLE = auto()
    DROP_TABLE = auto()
    ADD_COLUMN = auto()
    DROP_COLUMN = auto()
    ALTER_COLUMN = auto()
    ADD_INDEX = auto()
    DROP_INDEX = auto()
    CREATE_SCHEMA = auto()


###
##      SCHEMA CHANGE
####
@dataclass
class SchemaChange:
    """A single DDL operation that needs to be applied to the database.

    Produced by ``diff_states()`` and consumed by ``MigrationRunner``.

    Attributes:
        kind:        What kind of change this is.
        table:       The table being modified.
        schema:      Database schema (PostgreSQL). Empty = default.
        column:      The column being modified (None for table-level changes).
        old_state:   The before-state (None for CREATE operations).
        new_state:   The after-state (None for DROP operations).
        description: Human-readable description for migration output.
    """
    kind: ChangeKind
    table: str
    schema: str = ""
    column: Optional[str] = None
    old_state: Optional[ColumnState] = None
    new_state: Optional[ColumnState] = None
    description: str = ""

    def __str__(self) -> str:
        return self.description or f"{self.kind.name} on {self.table}"


####    Diff engine
def diff_states(current: SchemaState, target: SchemaState) -> List[SchemaChange]:
    """Compute the list of changes needed to bring ``current`` to ``target``.

    Tables are matched by composite key ``(schema, name)``. Schemas that
    exist in the target but not in the current state get a ``CREATE_SCHEMA``
    change (for PostgreSQL multi-schema support).
    """
    changes: List[SchemaChange] = []

    # Detect schemas in target but not in current → CreateSchema
    target_schemas = {t.schema for t in target.tables.values()}
    current_schemas = {t.schema for t in current.tables.values()}
    for schema in target_schemas - current_schemas:
        if schema:
            changes.append(SchemaChange(
                kind=ChangeKind.CREATE_SCHEMA,
                table="",
                schema=schema,
                description=f"Create schema '{schema}'",
            ))

    # Helper: find table by composite key
    def _find(state, table_name, schema):
        for t in state.tables.values():
            if t.name == table_name and (t.schema or "") == (schema or ""):
                return t
        return None

    # Tables to create (in target but not in current)
    for table_name, target_table in target.tables.items():
        current_table = _find(current, table_name, target_table.schema)
        if current_table is None:
            changes.append(SchemaChange(
                kind=ChangeKind.CREATE_TABLE,
                table=table_name,
                schema=target_table.schema,
                new_state=None,
                description=f"Create table '{table_name}' in schema '{target_table.schema or 'default'}'",
            ))
            # All columns in this new table are implicitly "added" by CREATE TABLE
            continue

        # Columns to add or alter
        for col_name, target_col in target_table.columns.items():
            if not current_table.has_column(col_name):
                changes.append(SchemaChange(
                    kind=ChangeKind.ADD_COLUMN,
                    table=table_name,
                    schema=target_table.schema,
                    column=col_name,
                    new_state=target_col,
                    description=f"Add column '{col_name}' to '{table_name}'",
                ))
            else:
                current_col = current_table.columns[col_name]
                if current_col != target_col:
                    changes.append(SchemaChange(
                        kind=ChangeKind.ALTER_COLUMN,
                        table=table_name,
                        schema=target_table.schema,
                        column=col_name,
                        old_state=current_col,
                        new_state=target_col,
                        description=(
                            f"Alter column '{col_name}' on '{table_name}': "
                            f"{current_col.db_type} → {target_col.db_type}"
                        ),
                    ))

    return changes


def project_state_from_models(models: list) -> SchemaState:
    """Build a SchemaState from a list of Model classes.

    This is the "what the code says the schema should be" side of the diff.

    Args:
        models: A list of Model subclasses to inspect.

    Returns:
        A SchemaState representing the schema implied by the given models.
    """
    state = SchemaState()

    for model in models:
        if not hasattr(model, "_meta"):
            continue

        schema = getattr(model._meta, "schema", "")
        table = TableState(name=model._meta.table_name, schema=schema)
        for field_name, f in model._meta.fields.items():
            col = ColumnState(
                name = f.column,
                db_type = f.db_type(),
                nullable = f.null,
                primary_key = f.primary_key,
                unique = f.unique or f.primary_key,
                default = f.get_default(),
            )
            table.add_column(col)
        state.add_table(table)

    return state
