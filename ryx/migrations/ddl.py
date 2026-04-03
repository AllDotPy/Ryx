"""
Ryx ORM — DDL Generator  (backend-aware)

Generates complete CREATE TABLE / ALTER TABLE / CREATE INDEX / DROP INDEX
SQL statements from SchemaState objects and SchemaChange diffs.

Backend differences handled here:
  Postgres : SERIAL PRIMARY KEY, BOOLEAN, UUID, JSONB, TIMESTAMP, ILIKE
  MySQL    : INT AUTO_INCREMENT PRIMARY KEY, TINYINT(1), TEXT not VARCHAR(>65535),
             DATETIME instead of TIMESTAMP, no UUID native type
  SQLite   : INTEGER PRIMARY KEY AUTOINCREMENT, no UUID, no JSONB,
             no ALTER COLUMN (requires table rebuild)

Usage:
  from ryx.migrations.ddl import DDLGenerator
  gen = DDLGenerator(backend="postgres")
  sql = gen.create_table(table_state)
  sql = gen.add_column(table_name, column_state)
  sql = gen.create_index(table_name, index)

"""
from __future__ import annotations

from typing import List, Optional, TYPE_CHECKING

if TYPE_CHECKING:
    from ryx.migrations.state import ColumnState, TableState
    from ryx.models import Index, Constraint


####    Backend detection
def detect_backend(url: str) -> str:
    """Detect the database backend from a connection URL string.

    Returns one of: "postgres", "mysql", "sqlite".
    Defaults to "postgres" for unknown URLs.
    """
    url_lower = url.lower()
    if url_lower.startswith("sqlite"):
        return "sqlite"
    if url_lower.startswith("mysql") or url_lower.startswith("mariadb"):
        return "mysql"
    return "postgres"


#### 
##      DDL GENERATOR
##### 
class DDLGenerator:
    """Generate DDL SQL statements for a specific database backend.

    Args:
        backend: One of "postgres" (default), "mysql", "sqlite".
    """

    def __init__(self, backend: str = "postgres") -> None:
        self.backend = backend.lower()

    # CREATE TABLE
    def create_table(self, table: "TableState") -> str:
        """Generate a CREATE TABLE IF NOT EXISTS statement.

        Includes all columns, the primary key, UNIQUE constraints, and
        backend-specific type translations.

        Args:
            table: The TableState describing all columns.

        Returns:
            A complete CREATE TABLE SQL string.
        """
        col_defs: List[str] = []
        for col in table.columns.values():
            col_defs.append(self._column_def(col))

        # Multi-column UNIQUE constraints (from unique_together)
        for uc in getattr(table, "unique_together", []):
            cols = ", ".join(self._q(c) for c in uc)
            col_defs.append(f"UNIQUE ({cols})")

        cols_sql = ",\n    ".join(col_defs)
        return (
            f"CREATE TABLE IF NOT EXISTS {self._q(table.name)} (\n"
            f"    {cols_sql}\n"
            f")"
        )

    # ALTER TABLE ADD COLUMN
    def add_column(self, table_name: str, col: "ColumnState") -> str:
        """Generate an ALTER TABLE ... ADD COLUMN statement.

        Args:
            table_name: The table to alter.
            col:        The ColumnState describing the new column.
        """
        col_def = self._column_def(col)
        return f"ALTER TABLE {self._q(table_name)} ADD COLUMN {col_def}"

    # ALTER TABLE ALTER COLUMN
    def alter_column(self, table_name: str, col: "ColumnState") -> Optional[str]:
        """Generate an ALTER COLUMN statement (Postgres/MySQL only).

        SQLite does not support ALTER COLUMN. Returns None for SQLite and
        logs a warning — the caller should handle this as a no-op or trigger
        a table rebuild.

        Args:
            table_name: The table containing the column.
            col:        The new ColumnState to apply.
        """
        if self.backend == "sqlite":
            # SQLite: ALTER COLUMN unsupported — caller must do table rebuild
            return None

        if self.backend == "mysql":
            # MySQL syntax: ALTER TABLE t MODIFY COLUMN col_def
            col_def = self._column_def(col)
            return f"ALTER TABLE {self._q(table_name)} MODIFY COLUMN {col_def}"

        # PostgreSQL: split into two statements (type change + nullability)
        db_type = self._translate_type(col.db_type)
        null_clause = "DROP NOT NULL" if col.nullable else "SET NOT NULL"
        return (
            f"ALTER TABLE {self._q(table_name)} "
            f"ALTER COLUMN {self._q(col.name)} TYPE {db_type}, "
            f"ALTER COLUMN {self._q(col.name)} {null_clause}"
        )

    # DROP COLUMN
    def drop_column(self, table_name: str, col_name: str) -> Optional[str]:
        """Generate a DROP COLUMN statement.

        SQLite does not support DROP COLUMN prior to v3.35.0.
        We generate the statement anyway and let the driver error if unsupported.
        """
        return (
            f"ALTER TABLE {self._q(table_name)} "
            f"DROP COLUMN {self._q(col_name)}"
        )

    # DROP TABLE 
    def drop_table(self, table_name: str) -> str:
        """Generate a DROP TABLE IF EXISTS statement."""
        return f"DROP TABLE IF EXISTS {self._q(table_name)}"

    # CREATE INDEX
    def create_index(self, table_name: str, index: "Index") -> str:
        """Generate a CREATE INDEX statement from an Index declaration.

        Args:
            table_name: The table the index belongs to.
            index:      An Index instance (fields, name, unique).

        Returns:
            A CREATE [UNIQUE] INDEX ... ON ... statement.
        """
        unique = "UNIQUE " if index.unique else ""
        cols   = ", ".join(self._q(f) for f in index.fields)
        return (
            f"CREATE {unique}INDEX IF NOT EXISTS {self._q(index.name)} "
            f"ON {self._q(table_name)} ({cols})"
        )

    def create_index_from_fields(
        self,
        table_name: str,
        fields:     List[str],
        name:       str,
        unique:     bool = False,
    ) -> str:
        """Generate a CREATE INDEX from a plain list of field names.

        Convenience method for ``index_together`` entries which are tuples
        of field names rather than Index objects.
        """
        unique_kw = "UNIQUE " if unique else ""
        cols = ", ".join(self._q(f) for f in fields)
        return (
            f"CREATE {unique_kw}INDEX IF NOT EXISTS {self._q(name)} "
            f"ON {self._q(table_name)} ({cols})"
        )

    # DROP INDEX
    def drop_index(self, index_name: str, table_name: str = "") -> str:
        """Generate a DROP INDEX statement.

        MySQL requires the table name; Postgres and SQLite do not.
        """
        if self.backend == "mysql" and table_name:
            return f"DROP INDEX {self._q(index_name)} ON {self._q(table_name)}"
        return f"DROP INDEX IF EXISTS {self._q(index_name)}"

    # ADD CONSTRAINT (CHECK)
    def add_constraint(self, table_name: str, constraint: "Constraint") -> Optional[str]:
        """Generate ADD CONSTRAINT ... CHECK (...) statement.

        SQLite supports CHECK constraints only in CREATE TABLE, not ALTER TABLE.
        Returns None for SQLite.
        """
        if self.backend == "sqlite":
            return None  # SQLite: include in CREATE TABLE only
        return (
            f"ALTER TABLE {self._q(table_name)} "
            f"ADD CONSTRAINT {self._q(constraint.name)} "
            f"CHECK ({constraint.check})"
        )

    # FOREIGN KEY
    def add_foreign_key(
        self,
        table_name: str,
        col_name: str,
        ref_table: str,
        ref_col: str,
        on_delete: str = "CASCADE",
        constraint_name: Optional[str] = None,
    ) -> Optional[str]:
        """Generate ADD FOREIGN KEY constraint DDL.

        SQLite only supports FK constraints at CREATE TABLE time.
        Returns None for SQLite inline mode.
        """
        if self.backend == "sqlite":
            return None  # FK constraints are inline in SQLite CREATE TABLE

        cname = constraint_name or f"fk_{table_name}_{col_name}"
        return (
            f"ALTER TABLE {self._q(table_name)} "
            f"ADD CONSTRAINT {self._q(cname)} "
            f"FOREIGN KEY ({self._q(col_name)}) "
            f"REFERENCES {self._q(ref_table)} ({self._q(ref_col)}) "
            f"ON DELETE {on_delete}"
        )

    # Internal: column definition
    def _column_def(self, col: "ColumnState") -> str:
        """Return the SQL column definition fragment for a single ColumnState.

        Applies backend-specific type translation and constraint keywords.
        """
        parts: List[str] = [self._q(col.name)]
        db_type = self._translate_type(col.db_type)

        # Auto-increment PK: each backend has its own syntax
        if col.primary_key and db_type.upper() in ("INTEGER", "BIGINT", "SMALLINT"):
            parts.append(self._serial_type(db_type))
            parts.append("PRIMARY KEY")
        else:
            parts.append(db_type)
            if col.primary_key:
                parts.append("PRIMARY KEY")
            if not col.nullable and not col.primary_key:
                parts.append("NOT NULL")
            if col.unique and not col.primary_key:
                parts.append("UNIQUE")
            if col.default is not None:
                parts.append(f"DEFAULT {col.default}")

        return " ".join(parts)

    def _serial_type(self, db_type: str) -> str:
        """Return the auto-increment type token for this backend."""
        dt = db_type.upper()
        if self.backend == "postgres":
            if dt == "BIGINT":   
                return "BIGSERIAL"
            if dt == "SMALLINT": 
                return "SMALLSERIAL"
            return "SERIAL"
        if self.backend == "mysql":
            return f"{dt} AUTO_INCREMENT"
        # SQLite
        return "INTEGER"  # SQLite uses "INTEGER PRIMARY KEY" without AUTOINCREMENT

    def _translate_type(self, db_type: str) -> str:
        """Translate a generic type string to a backend-specific SQL type.

        We store generic types in ColumnState (e.g. "VARCHAR(200)", "BOOLEAN",
        "UUID", "JSONB") and translate them here for each backend.
        """
        dt = db_type.upper().strip()

        if self.backend == "mysql":
            if dt == "BOOLEAN":           
                return "TINYINT(1)"
            if dt == "UUID":              
                return "CHAR(36)"
            if dt == "JSONB":             
                return "JSON"
            if dt == "TIMESTAMP":         
                return "DATETIME"
            if dt == "DOUBLE PRECISION":  
                return "DOUBLE"
            if dt == "BYTEA":             
                return "BLOB"

        if self.backend == "sqlite":
            if dt == "BOOLEAN":           
                return "INTEGER"
            if dt in ("UUID", "JSONB"):   
                return "TEXT"
            if dt == "TIMESTAMP":         
                return "TEXT"
            if dt.startswith("VARCHAR"):  
                return "TEXT"
            if dt == "DOUBLE PRECISION":  
                return "REAL"
            if dt == "BIGINT":            
                return "INTEGER"
            if dt == "SMALLINT":          
                return "INTEGER"
            if dt == "BYTEA":             
                return "BLOB"

        # Postgres (and default) — return as-is (these are native PG types)
        return db_type

    @staticmethod
    def _q(identifier: str) -> str:
        """Double-quote a SQL identifier."""
        return f'"{identifier.replace(chr(34), chr(34)*2)}"'


####    Convenience: generate all DDL for a full project state
def generate_schema_ddl(
    models: list,
    backend: str = "postgres",
    include_indexes: bool = True,
    include_constraints: bool = True,
) -> List[str]:
    """Generate the full list of DDL statements to create a fresh schema.

    Args:
        models:              List of Model subclasses.
        backend:             Target database backend.
        include_indexes:     If True, include CREATE INDEX for all declared indexes.
        include_constraints: If True, include CHECK constraints (where supported).

    Returns:
        An ordered list of SQL strings ready to execute.
    """
    from ryx.migrations.state import project_state_from_models
    from ryx.models import Index, Constraint

    gen   = DDLGenerator(backend)
    state = project_state_from_models(models)
    stmts: List[str] = []

    for table in state.tables.values():
        stmts.append(gen.create_table(table))

    if not include_indexes:
        return stmts

    # CREATE INDEX for each model's declared indexes and index_together
    for model in models:
        if not hasattr(model, "_meta"):
            continue
        meta = model._meta
        table = meta.table_name

        # Named indexes from Meta.indexes
        for idx in meta.indexes:
            stmts.append(gen.create_index(table, idx))

        # index_together (legacy syntax)
        for i, fields in enumerate(meta.index_together):
            name = f"idx_{table}_{'_'.join(fields)}_{i}"
            stmts.append(gen.create_index_from_fields(table, list(fields), name))

        # unique_together → UNIQUE INDEX
        for i, fields in enumerate(meta.unique_together):
            name = f"uq_{table}_{'_'.join(fields)}_{i}"
            stmts.append(gen.create_index_from_fields(table, list(fields), name, unique=True))

        # CHECK constraints
        if include_constraints:
            for constraint in meta.constraints:
                sql = gen.add_constraint(table, constraint)
                if sql:
                    stmts.append(sql)

    return stmts
