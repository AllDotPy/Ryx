use ryx_backend::backends::{DecodedRow, RyxBackend};
use ryx_common::RyxResult;
use ryx_query::Backend;
use serde::{Deserialize, Serialize};

use crate::model::{FieldMeta, Model};

pub mod operations;
pub use operations::*;
pub mod ddl;
pub use ddl::*;
#[cfg(feature = "config-yaml")]
pub mod files;
#[cfg(feature = "config-yaml")]
pub use files::*;
pub mod autodetect;
pub use autodetect::*;
pub mod runner;
pub use runner::*;

// ============================================================
// State types
// ============================================================

/// A snapshot of a single database column, as seen in the live DB
/// or as declared by a model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColumnState {
    pub name: String,
    pub db_type: String,
    pub nullable: bool,
    pub primary_key: bool,
    pub unique: bool,
    pub default: Option<String>,
}

impl From<&FieldMeta> for ColumnState {
    fn from(m: &FieldMeta) -> Self {
        Self {
            name: m.name.to_string(),
            db_type: m.db_type.to_string(),
            nullable: m.nullable,
            primary_key: m.primary_key,
            unique: m.unique,
            default: m.default.map(|s| s.to_string()),
        }
    }
}

/// A snapshot of a single database table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableState {
    pub name: String,
    /// Database schema this table belongs to (empty string = default / no schema).
    #[serde(default)]
    pub schema: String,
    pub columns: Vec<ColumnState>,
}

/// A full schema as known by the database or by the model declarations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SchemaState {
    pub tables: Vec<TableState>,
}

// ============================================================
// Change / diff types
// ============================================================

/// The kind of schema change to perform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    CreateTable,
    DropTable,
    AddColumn,
    DropColumn,
    AlterColumn,
    CreateIndex,
    DropIndex,
    /// Create a new database schema (e.g. ``CREATE SCHEMA IF NOT EXISTS "tenant1"``).
    CreateSchema,
}

/// A single schema change operation.
#[derive(Debug, Clone, PartialEq)]
pub struct SchemaChange {
    pub kind: ChangeKind,
    pub table: String,
    /// Database schema this change applies to (empty = default / no qualification).
    pub schema: String,
    pub column: Option<ColumnState>,
    pub old_column: Option<ColumnState>,
    /// Human-readable description of the change.
    pub description: String,
}

/// Compare two schema states and return the list of changes needed to
/// go from `current` to `target`.
///
/// Tables are matched by composite key `(schema, name)`. Schemas that exist
/// in the target but not in the current state get a `CreateSchema` change.
pub fn diff_states(current: &SchemaState, target: &SchemaState) -> Vec<SchemaChange> {
    let mut changes = Vec::new();

    // Collect schemas in both states
    let target_schemas: std::collections::BTreeSet<&str> =
        target.tables.iter().map(|t| t.schema.as_str()).collect();
    let current_schemas: std::collections::BTreeSet<&str> =
        current.tables.iter().map(|t| t.schema.as_str()).collect();

    // Schemas in target but not in current → CreateSchema
    for schema in target_schemas.difference(&current_schemas) {
        if !schema.is_empty() {
            changes.push(SchemaChange {
                kind: ChangeKind::CreateSchema,
                table: String::new(),
                schema: schema.to_string(),
                column: None,
                old_column: None,
                description: format!("Create schema {schema}"),
            });
        }
    }

    // Tables in target but not in current → CREATE
    for table in &target.tables {
        let exists = current
            .tables
            .iter()
            .any(|t| t.schema == table.schema && t.name == table.name);
        if !exists {
            changes.push(SchemaChange {
                kind: ChangeKind::CreateTable,
                table: table.name.clone(),
                schema: table.schema.clone(),
                column: None,
                old_column: None,
                description: format!(
                    "Create table {}.{}",
                    table.schema, table.name
                ),
            });
        }
    }

    // Columns of newly created tables → also emit AddColumn (so generate_ddl can use them)
    for table in &target.tables {
        let exists = current
            .tables
            .iter()
            .any(|t| t.schema == table.schema && t.name == table.name);
        if exists {
            continue;
        }
        for col in &table.columns {
            changes.push(SchemaChange {
                kind: ChangeKind::AddColumn,
                table: table.name.clone(),
                schema: table.schema.clone(),
                column: Some(col.clone()),
                old_column: None,
                description: format!(
                    "Add column {}.{}.{}",
                    table.schema, table.name, col.name
                ),
            });
        }
    }

    // Columns in target but not in current → ADD COLUMN
    for table in &target.tables {
        if let Some(current_table) = current
            .tables
            .iter()
            .find(|t| t.schema == table.schema && t.name == table.name)
        {
            let current_names: Vec<&str> =
                current_table.columns.iter().map(|c| c.name.as_str()).collect();
            for col in &table.columns {
                if !current_names.contains(&col.name.as_str()) {
                    changes.push(SchemaChange {
                        kind: ChangeKind::AddColumn,
                        table: table.name.clone(),
                        schema: table.schema.clone(),
                        column: Some(col.clone()),
                        old_column: None,
                        description: format!(
                            "Add column {}.{}.{}",
                            table.schema, table.name, col.name
                        ),
                    });
                }
            }
        }
    }

    // Columns in both but different → ALTER COLUMN
    for table in &target.tables {
        if let Some(current_table) = current
            .tables
            .iter()
            .find(|t| t.schema == table.schema && t.name == table.name)
        {
            for col in &table.columns {
                if let Some(current_col) =
                    current_table.columns.iter().find(|c| c.name == col.name)
                {
                    if col != current_col {
                        changes.push(SchemaChange {
                            kind: ChangeKind::AlterColumn,
                            table: table.name.clone(),
                            schema: table.schema.clone(),
                            column: Some(col.clone()),
                            old_column: Some(current_col.clone()),
                            description: format!(
                                "Alter column {}.{}.{}",
                                table.schema, table.name, col.name
                            ),
                        });
                    }
                }
            }
        }
    }

    changes
}

// ============================================================
// DDL Generator (thin wrapper for backward compat)
// ============================================================

/// Generate DDL statements for the given changes on the specified backend.
///
/// Delegates to [`DDLGenerator::generate`].
pub fn generate_ddl(changes: &[SchemaChange], backend: Backend) -> Vec<String> {
    DDLGenerator::new(backend).generate(changes)
}

// ============================================================
// Introspection
// ============================================================

pub const MIGRATIONS_TABLE: &str = "ryx_migrations";

/// Introspect the live database and return its current `SchemaState`.
///
/// ``schema`` filters by database schema for backends that support it
/// (PostgreSQL). For MySQL and SQLite the parameter is ignored.
pub async fn introspect_schema(
    backend: &dyn RyxBackend,
    backend_type: Backend,
    schema: &str,
) -> RyxResult<SchemaState> {
    match backend_type {
        Backend::PostgreSQL => introspect_schema_postgres(backend, schema).await,
        Backend::MySQL => introspect_schema_mysql(backend).await,
        Backend::SQLite => introspect_schema_sqlite(backend).await,
    }
}

// ── SQLite ───────────────────────────────────────────────────

async fn introspect_schema_sqlite(backend: &dyn RyxBackend) -> RyxResult<SchemaState> {
    let table_rows = backend
        .fetch_raw(
            "SELECT name FROM sqlite_master WHERE type = 'table' \
             AND name NOT LIKE 'sqlite_%' AND name != 'ryx_migrations'"
                .to_string(),
            None,
        )
        .await?;

    let mut tables = Vec::new();
    for row in &table_rows {
        let table_name = get_text(row, "name").unwrap_or_default();
        let columns = introspect_columns_sqlite(backend, &table_name).await?;
        tables.push(TableState {
            name: table_name,
            schema: String::new(),
            columns,
        });
    }
    Ok(SchemaState { tables })
}

async fn introspect_columns_sqlite(
    backend: &dyn RyxBackend,
    table: &str,
) -> RyxResult<Vec<ColumnState>> {
    let sql = format!("PRAGMA table_info(\"{table}\")");
    let rows = backend.fetch_raw(sql, None).await?;
    let mut columns = Vec::new();
    for row in &rows {
        let name = get_text(row, "name").unwrap_or_default();
        let db_type = get_text(row, "type").unwrap_or_default();
        let not_null = get_int(row, "notnull").unwrap_or(0);
        let pk = get_int(row, "pk").unwrap_or(0);
        let dflt = get_text(row, "dflt_value");
        columns.push(ColumnState {
            name,
            db_type,
            nullable: not_null == 0,
            primary_key: pk != 0,
            unique: false,
            default: dflt,
        });
    }
    Ok(columns)
}

// ── PostgreSQL ───────────────────────────────────────────────

async fn introspect_schema_postgres(backend: &dyn RyxBackend, schema: &str) -> RyxResult<SchemaState> {
    let schema_clause = if schema.is_empty() {
        "table_schema = 'public'".to_string()
    } else {
        format!("table_schema = '{schema}'")
    };
    let sql = format!(
        "SELECT table_name, table_schema FROM information_schema.tables \
         WHERE {schema_clause} AND table_type = 'BASE TABLE' \
         AND table_name != 'ryx_migrations'"
    );
    let table_rows = backend.fetch_raw(sql, None).await?;

    let mut tables = Vec::new();
    for row in &table_rows {
        let table_name = get_text(row, "table_name").unwrap_or_default();
        let table_schema = get_text(row, "table_schema").unwrap_or_else(|| schema.to_string());
        let columns = introspect_columns_postgres(backend, &table_name, &table_schema).await?;
        tables.push(TableState {
            name: table_name,
            schema: table_schema,
            columns,
        });
    }
    Ok(SchemaState { tables })
}

async fn introspect_columns_postgres(
    backend: &dyn RyxBackend,
    table: &str,
    schema: &str,
) -> RyxResult<Vec<ColumnState>> {
    let sql = format!(
        "SELECT column_name, data_type, is_nullable, column_default \
         FROM information_schema.columns \
         WHERE table_schema = '{schema}' AND table_name = '{table}' \
         ORDER BY ordinal_position"
    );
    let rows = backend.fetch_raw(sql, None).await?;

    // Get primary key columns
    let pk_cols = get_constraint_columns_postgres(backend, table, schema, "PRIMARY KEY").await?;
    let unique_cols = get_constraint_columns_postgres(backend, table, schema, "UNIQUE").await?;

    let mut columns = Vec::new();
    for row in &rows {
        let name = get_text(row, "column_name").unwrap_or_default();
        let raw_type = get_text(row, "data_type").unwrap_or_default();
        let nullable = get_text(row, "is_nullable")
            .map(|v| v == "YES")
            .unwrap_or(true);
        let dflt = get_text(row, "column_default");
        let is_pk = pk_cols.contains(&name);
        let is_unique = unique_cols.contains(&name);

        columns.push(ColumnState {
            name,
            db_type: normalize_pg_type(&raw_type),
            nullable,
            primary_key: is_pk,
            unique: is_unique,
            default: dflt,
        });
    }
    Ok(columns)
}

async fn get_constraint_columns_postgres(
    backend: &dyn RyxBackend,
    table: &str,
    schema: &str,
    constraint_type: &str,
) -> RyxResult<Vec<String>> {
    let sql = format!(
        "SELECT kcu.column_name \
         FROM information_schema.table_constraints tc \
         JOIN information_schema.key_column_usage kcu \
           ON tc.constraint_name = kcu.constraint_name \
          AND tc.table_schema = kcu.table_schema \
          AND tc.table_name = kcu.table_name \
         WHERE tc.table_schema = '{schema}' \
           AND tc.table_name = '{table}' \
           AND tc.constraint_type = '{constraint_type}'"
    );
    let rows = backend.fetch_raw(sql, None).await?;
    Ok(rows.iter().filter_map(|r| get_text(r, "column_name")).collect())
}

fn normalize_pg_type(raw: &str) -> String {
    match raw {
        "integer" => "INTEGER".to_string(),
        "bigint" => "BIGINT".to_string(),
        "smallint" => "INTEGER".to_string(),
        "text" => "TEXT".to_string(),
        "boolean" => "BOOLEAN".to_string(),
        "double precision" => "DOUBLE PRECISION".to_string(),
        "real" => "REAL".to_string(),
        "numeric" | "decimal" => "DECIMAL".to_string(),
        "timestamp without time zone" | "timestamp" => "TIMESTAMP".to_string(),
        "timestamp with time zone" => "TIMESTAMPTZ".to_string(),
        "date" => "DATE".to_string(),
        "time without time zone" | "time" => "TIME".to_string(),
        "uuid" => "UUID".to_string(),
        "jsonb" => "JSONB".to_string(),
        "json" => "JSONB".to_string(),
        _ => raw.to_uppercase(),
    }
}

// ── MySQL ────────────────────────────────────────────────────

async fn introspect_schema_mysql(backend: &dyn RyxBackend) -> RyxResult<SchemaState> {
    let table_rows = backend
        .fetch_raw(
            "SELECT table_name FROM information_schema.tables \
             WHERE table_schema = DATABASE() AND table_type = 'BASE TABLE' \
             AND table_name != 'ryx_migrations'"
                .to_string(),
            None,
        )
        .await?;

    let mut tables = Vec::new();
    for row in &table_rows {
        let table_name = get_text(row, "table_name").unwrap_or_default();
        let columns = introspect_columns_mysql(backend, &table_name).await?;
        tables.push(TableState {
            name: table_name,
            schema: String::new(),
            columns,
        });
    }
    Ok(SchemaState { tables })
}

async fn introspect_columns_mysql(
    backend: &dyn RyxBackend,
    table: &str,
) -> RyxResult<Vec<ColumnState>> {
    let sql = format!(
        "SELECT column_name, data_type, is_nullable, column_default \
         FROM information_schema.columns \
         WHERE table_schema = DATABASE() AND table_name = '{table}' \
         ORDER BY ordinal_position"
    );
    let rows = backend.fetch_raw(sql, None).await?;

    let pk_cols = get_constraint_columns_mysql(backend, table, "PRIMARY KEY").await?;
    let unique_cols = get_constraint_columns_mysql(backend, table, "UNIQUE").await?;

    let mut columns = Vec::new();
    for row in &rows {
        let name = get_text(row, "column_name").unwrap_or_default();
        let raw_type = get_text(row, "data_type").unwrap_or_default();
        let nullable = get_text(row, "is_nullable")
            .map(|v| v == "YES")
            .unwrap_or(true);
        let dflt = get_text(row, "column_default");
        let is_pk = pk_cols.contains(&name);
        let is_unique = unique_cols.contains(&name);

        columns.push(ColumnState {
            name,
            db_type: normalize_mysql_type(&raw_type),
            nullable,
            primary_key: is_pk,
            unique: is_unique,
            default: dflt,
        });
    }
    Ok(columns)
}

async fn get_constraint_columns_mysql(
    backend: &dyn RyxBackend,
    table: &str,
    constraint_type: &str,
) -> RyxResult<Vec<String>> {
    let sql = format!(
        "SELECT kcu.column_name \
         FROM information_schema.table_constraints tc \
         JOIN information_schema.key_column_usage kcu \
           ON tc.constraint_name = kcu.constraint_name \
          AND tc.table_schema = kcu.table_schema \
          AND tc.table_name = kcu.table_name \
         WHERE tc.table_schema = DATABASE() \
           AND tc.table_name = '{table}' \
           AND tc.constraint_type = '{constraint_type}'"
    );
    let rows = backend.fetch_raw(sql, None).await?;
    Ok(rows.iter().filter_map(|r| get_text(r, "column_name")).collect())
}

fn normalize_mysql_type(raw: &str) -> String {
    match raw {
        "tinyint" | "tinyint(1)" => "BOOLEAN".to_string(),
        "int" | "integer" => "INTEGER".to_string(),
        "bigint" => "BIGINT".to_string(),
        "smallint" | "mediumint" => "INTEGER".to_string(),
        "double" => "DOUBLE PRECISION".to_string(),
        "float" | "real" => "REAL".to_string(),
        "text" | "tinytext" | "mediumtext" | "longtext" => "TEXT".to_string(),
        "varchar" | "char" => "TEXT".to_string(),
        "datetime" | "timestamp" => "TIMESTAMP".to_string(),
        "date" => "DATE".to_string(),
        "time" => "TIME".to_string(),
        "json" => "JSONB".to_string(),
        "decimal" | "numeric" => "DECIMAL".to_string(),
        _ => raw.to_uppercase(),
    }
}

// ── Helpers ──────────────────────────────────────────────────

pub(crate) fn get_text(row: &DecodedRow, key: &str) -> Option<String> {
    row.get(key).and_then(|v| match v {
        ryx_query::ast::SqlValue::Text(s) => Some(s.clone()),
        _ => None,
    })
}

pub(crate) fn get_int(row: &DecodedRow, key: &str) -> Option<i64> {
    row.get(key).and_then(|v| match v {
        ryx_query::ast::SqlValue::Int(n) => Some(*n),
        _ => None,
    })
}

// ============================================================
// Utilities
// ============================================================

/// Detect backend type from a database URL string.
///
/// ```ignore
/// use ryx_rs::migration::detect_backend;
/// use ryx_query::Backend;
///
/// assert_eq!(detect_backend("postgres://localhost/db"), Backend::PostgreSQL);
/// assert_eq!(detect_backend("mysql://localhost/db"), Backend::MySQL);
/// assert_eq!(detect_backend("sqlite::memory:"), Backend::SQLite);
/// ```
pub fn detect_backend(url: &str) -> Backend {
    if url.starts_with("postgres") || url.starts_with("postgresql") {
        Backend::PostgreSQL
    } else if url.starts_with("mysql") || url.starts_with("mariadb") {
        Backend::MySQL
    } else {
        Backend::SQLite
    }
}

// ============================================================
// Migration Runner (backward-compat wrapper)
// ============================================================

/// Apply migrations — introspect, diff, and execute DDL.
///
/// Now delegates to the file-based ``FileRunner`` from ``migration/runner``.
/// Supports file-based migrations with per-alias tracking, recursive
/// discovery, and an interactive fallback when no files exist.
///
/// ```ignore
/// use ryx_rs::migration::MigrationRunner;
///
/// MigrationRunner::new()
///     .model::<Post>()
///     .model::<Author>()
///     .run().await?;
///
/// // Multi-db
/// MigrationRunner::new()
///     .db("replica")
///     .model::<Post>()
///     .run().await?;
/// ```
pub struct MigrationRunner {
    inner: FileRunner,
}

impl MigrationRunner {
    pub fn new() -> Self {
        Self {
            inner: FileRunner::new(),
        }
    }

    pub fn db(mut self, alias: &str) -> Self {
        self.inner = self.inner.db(alias);
        self
    }

    pub fn migrations_dir(mut self, dir: &str) -> Self {
        self.inner = self.inner.migrations_dir(dir);
        self
    }

    pub fn dry_run(mut self, dry: bool) -> Self {
        self.inner = self.inner.dry_run(dry);
        self
    }

    pub fn no_interactive(mut self, no: bool) -> Self {
        self.inner = self.inner.no_interactive(no);
        self
    }

    pub fn live(mut self, live: bool) -> Self {
        self.inner = self.inner.live(live);
        self
    }

    /// Set the database schema for PostgreSQL multi-schema support.
    ///
    /// When set, all introspection, DDL, and operations scope to this schema.
    /// Leave empty for the default schema (no qualification).
    pub fn schema(mut self, schema: &str) -> Self {
        self.inner = self.inner.schema(schema);
        self
    }

    pub fn model<M: Model>(mut self) -> Self {
        self.inner = self.inner.model::<M>();
        self
    }

    pub async fn run(self) -> RyxResult<Vec<String>> {
        self.inner.run().await
    }

    pub async fn plan(self) -> RyxResult<Vec<String>> {
        self.inner.plan().await
    }
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use ryx_query::ast::SqlValue;

    // ── normalize_pg_type ──────────────────────────────────

    #[test]
    fn test_normalize_pg_type_known() {
        let cases = [
            ("integer", "INTEGER"),
            ("bigint", "BIGINT"),
            ("smallint", "INTEGER"),
            ("text", "TEXT"),
            ("boolean", "BOOLEAN"),
            ("double precision", "DOUBLE PRECISION"),
            ("real", "REAL"),
            ("numeric", "DECIMAL"),
            ("decimal", "DECIMAL"),
            ("timestamp without time zone", "TIMESTAMP"),
            ("timestamp with time zone", "TIMESTAMPTZ"),
            ("date", "DATE"),
            ("time without time zone", "TIME"),
            ("uuid", "UUID"),
            ("jsonb", "JSONB"),
            ("json", "JSONB"),
        ];
        for (raw, expected) in &cases {
            assert_eq!(normalize_pg_type(raw), *expected, "PG type {raw}");
        }
    }

    #[test]
    fn test_normalize_pg_type_unknown_uppercased() {
        assert_eq!(normalize_pg_type("tinyint"), "TINYINT");
        assert_eq!(normalize_pg_type("my_custom_type"), "MY_CUSTOM_TYPE");
    }

    // ── normalize_mysql_type ───────────────────────────────

    #[test]
    fn test_normalize_mysql_type_known() {
        let cases = [
            ("tinyint", "BOOLEAN"),
            ("tinyint(1)", "BOOLEAN"),
            ("int", "INTEGER"),
            ("integer", "INTEGER"),
            ("bigint", "BIGINT"),
            ("smallint", "INTEGER"),
            ("mediumint", "INTEGER"),
            ("double", "DOUBLE PRECISION"),
            ("float", "REAL"),
            ("real", "REAL"),
            ("text", "TEXT"),
            ("tinytext", "TEXT"),
            ("mediumtext", "TEXT"),
            ("longtext", "TEXT"),
            ("varchar", "TEXT"),
            ("char", "TEXT"),
            ("datetime", "TIMESTAMP"),
            ("timestamp", "TIMESTAMP"),
            ("date", "DATE"),
            ("time", "TIME"),
            ("json", "JSONB"),
            ("decimal", "DECIMAL"),
            ("numeric", "DECIMAL"),
        ];
        for (raw, expected) in &cases {
            assert_eq!(normalize_mysql_type(raw), *expected, "MySQL type {raw}");
        }
    }

    // ── col_type_for_backend ───────────────────────────────

    #[test]
    fn test_col_type_sqlite() {
        assert_eq!(col_type_for_backend("BOOLEAN", Backend::SQLite), "INTEGER");
        assert_eq!(col_type_for_backend("INTEGER", Backend::SQLite), "INTEGER");
        assert_eq!(col_type_for_backend("BIGINT", Backend::SQLite), "INTEGER");
        assert_eq!(col_type_for_backend("DOUBLE PRECISION", Backend::SQLite), "REAL");
        assert_eq!(col_type_for_backend("REAL", Backend::SQLite), "REAL");
        assert_eq!(col_type_for_backend("TEXT", Backend::SQLite), "TEXT");
        assert_eq!(col_type_for_backend("UUID", Backend::SQLite), "TEXT");
        assert_eq!(col_type_for_backend("JSONB", Backend::SQLite), "TEXT");
    }

    #[test]
    fn test_col_type_postgres() {
        assert_eq!(col_type_for_backend("BOOLEAN", Backend::PostgreSQL), "BOOLEAN");
        assert_eq!(col_type_for_backend("INTEGER", Backend::PostgreSQL), "INTEGER");
        assert_eq!(col_type_for_backend("BIGINT", Backend::PostgreSQL), "BIGINT");
        assert_eq!(col_type_for_backend("DOUBLE PRECISION", Backend::PostgreSQL), "DOUBLE PRECISION");
        assert_eq!(col_type_for_backend("TIMESTAMP", Backend::PostgreSQL), "TIMESTAMP");
        assert_eq!(col_type_for_backend("UUID", Backend::PostgreSQL), "UUID");
        assert_eq!(col_type_for_backend("JSONB", Backend::PostgreSQL), "JSONB");
    }

    #[test]
    fn test_col_type_mysql() {
        assert_eq!(col_type_for_backend("BOOLEAN", Backend::MySQL), "TINYINT(1)");
        assert_eq!(col_type_for_backend("INTEGER", Backend::MySQL), "INT");
        assert_eq!(col_type_for_backend("BIGINT", Backend::MySQL), "BIGINT");
        assert_eq!(col_type_for_backend("DOUBLE PRECISION", Backend::MySQL), "DOUBLE");
        assert_eq!(col_type_for_backend("REAL", Backend::MySQL), "FLOAT");
        assert_eq!(col_type_for_backend("TEXT", Backend::MySQL), "TEXT");
        assert_eq!(col_type_for_backend("TIMESTAMP", Backend::MySQL), "DATETIME");
        assert_eq!(col_type_for_backend("UUID", Backend::MySQL), "CHAR(36)");
        assert_eq!(col_type_for_backend("JSONB", Backend::MySQL), "JSON");
    }

    // ── build_col_sql ──────────────────────────────────────

    #[test]
    fn test_build_col_sql_simple() {
        let col = ColumnState {
            name: "title".into(),
            db_type: "TEXT".into(),
            nullable: false,
            primary_key: false,
            unique: false,
            default: None,
        };
        let sql = build_col_sql(&col, Backend::PostgreSQL, false);
        assert_eq!(sql, r#""title" TEXT NOT NULL"#);
    }

    #[test]
    fn test_build_col_sql_nullable() {
        let col = ColumnState {
            name: "bio".into(),
            db_type: "TEXT".into(),
            nullable: true,
            primary_key: false,
            unique: false,
            default: None,
        };
        let sql = build_col_sql(&col, Backend::PostgreSQL, false);
        assert_eq!(sql, r#""bio" TEXT"#);
    }

    #[test]
    fn test_build_col_sql_unique() {
        let col = ColumnState {
            name: "email".into(),
            db_type: "TEXT".into(),
            nullable: false,
            primary_key: false,
            unique: true,
            default: None,
        };
        let sql = build_col_sql(&col, Backend::PostgreSQL, false);
        assert_eq!(sql, r#""email" TEXT NOT NULL UNIQUE"#);
    }

    #[test]
    fn test_build_col_sql_default() {
        let col = ColumnState {
            name: "score".into(),
            db_type: "INTEGER".into(),
            nullable: false,
            primary_key: false,
            unique: false,
            default: Some("0".into()),
        };
        let sql = build_col_sql(&col, Backend::PostgreSQL, false);
        assert_eq!(sql, r#""score" INTEGER NOT NULL DEFAULT 0"#);
    }

    #[test]
    fn test_build_col_sql_pk_sqlite() {
        let col = ColumnState {
            name: "id".into(),
            db_type: "INTEGER".into(),
            nullable: false,
            primary_key: true,
            unique: false,
            default: None,
        };
        let sql = build_col_sql(&col, Backend::SQLite, true);
        assert_eq!(sql, r#""id" INTEGER PRIMARY KEY AUTOINCREMENT"#);
    }

    #[test]
    fn test_build_col_sql_pk_postgres() {
        let col = ColumnState {
            name: "id".into(),
            db_type: "BIGINT".into(),
            nullable: false,
            primary_key: true,
            unique: false,
            default: None,
        };
        let sql = build_col_sql(&col, Backend::PostgreSQL, true);
        assert_eq!(sql, r#""id" BIGSERIAL"#);
    }

    #[test]
    fn test_build_col_sql_pk_mysql() {
        let col = ColumnState {
            name: "id".into(),
            db_type: "INTEGER".into(),
            nullable: false,
            primary_key: true,
            unique: false,
            default: None,
        };
        let sql = build_col_sql(&col, Backend::MySQL, true);
        assert_eq!(sql, r#""id" INT AUTO_INCREMENT"#);
    }

    // ── diff_states ────────────────────────────────────────

    fn table(name: &str, cols: &[(&str, &str, bool)]) -> TableState {
        TableState {
            name: name.to_string(),
            schema: String::new(),
            columns: cols
                .iter()
                .map(|(n, t, pk)| ColumnState {
                    name: n.to_string(),
                    db_type: t.to_string(),
                    nullable: false,
                    primary_key: *pk,
                    unique: false,
                    default: None,
                })
                .collect(),
        }
    }

    #[test]
    fn test_diff_empty_to_table() {
        let current = SchemaState { tables: vec![] };
        let target = SchemaState {
            tables: vec![table("posts", &[("id", "INTEGER", true), ("title", "TEXT", false)])],
        };
        let changes = diff_states(&current, &target);
        assert_eq!(changes.len(), 3); // CreateTable + 2 AddColumn
        assert!(changes.iter().any(|c| c.kind == ChangeKind::CreateTable));
        assert_eq!(
            changes.iter().filter(|c| c.kind == ChangeKind::AddColumn).count(),
            2
        );
    }

    #[test]
    fn test_diff_add_column() {
        let current = SchemaState {
            tables: vec![table("posts", &[("id", "INTEGER", true)])],
        };
        let target = SchemaState {
            tables: vec![table("posts", &[("id", "INTEGER", true), ("title", "TEXT", false)])],
        };
        let changes = diff_states(&current, &target);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, ChangeKind::AddColumn);
        assert_eq!(changes[0].column.as_ref().unwrap().name, "title");
    }

    #[test]
    fn test_diff_alter_column() {
        let current = SchemaState {
            tables: vec![table("posts", &[("id", "INTEGER", true), ("title", "TEXT", false)])],
        };
        let target = SchemaState {
            tables: vec![table("posts", &[("id", "INTEGER", true), ("title", "VARCHAR", false)])],
        };
        let changes = diff_states(&current, &target);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, ChangeKind::AlterColumn);
    }

    #[test]
    fn test_diff_noop() {
        let current = SchemaState {
            tables: vec![table("posts", &[("id", "INTEGER", true)])],
        };
        let target = SchemaState {
            tables: vec![table("posts", &[("id", "INTEGER", true)])],
        };
        let changes = diff_states(&current, &target);
        assert!(changes.is_empty());
    }

    #[test]
    fn test_diff_multiple_tables() {
        let current = SchemaState { tables: vec![] };
        let target = SchemaState {
            tables: vec![
                table("posts", &[("id", "INTEGER", true)]),
                table("authors", &[("id", "INTEGER", true), ("name", "TEXT", false)]),
            ],
        };
        let changes = diff_states(&current, &target);
        assert_eq!(changes.len(), 5); // 2 CreateTable + 3 AddColumn (1 for posts + 2 for authors)
        assert_eq!(
            changes.iter().filter(|c| c.kind == ChangeKind::CreateTable).count(),
            2
        );
    }

    // ── generate_ddl ───────────────────────────────────────

    #[test]
    fn test_ddl_create_table_sqlite() {
        let changes = vec![
            SchemaChange {
                kind: ChangeKind::CreateTable,
                table: "posts".into(),
                schema: String::new(),
                column: None,
                old_column: None,
            description: String::new(),
            },
            SchemaChange {
                kind: ChangeKind::AddColumn,
                table: "posts".into(),
                schema: String::new(),
                column: Some(ColumnState {
                    name: "id".into(),
                    db_type: "INTEGER".into(),
                    nullable: false,
                    primary_key: true,
                    unique: false,
                    default: None,
                }),
                old_column: None,
            description: String::new(),
            },
            SchemaChange {
                kind: ChangeKind::AddColumn,
                table: "posts".into(),
                schema: String::new(),
                column: Some(ColumnState {
                    name: "title".into(),
                    db_type: "TEXT".into(),
                    nullable: false,
                    primary_key: false,
                    unique: false,
                    default: None,
                }),
                old_column: None,
            description: String::new(),
            },
        ];
        let sql = generate_ddl(&changes, Backend::SQLite);
        assert_eq!(sql.len(), 1);
        assert!(sql[0].contains("CREATE TABLE"));
        assert!(sql[0].contains("id"));
        assert!(sql[0].contains("title"));
        // SQLite PK should use AUTOINCREMENT (not PRIMARY KEY constraint separate)
        assert!(sql[0].contains("INTEGER PRIMARY KEY AUTOINCREMENT"));
    }

    #[test]
    fn test_ddl_create_table_postgres() {
        let changes = vec![
            SchemaChange {
                kind: ChangeKind::CreateTable,
                table: "posts".into(),
                schema: String::new(),
                column: None,
                old_column: None,
            description: String::new(),
            },
            SchemaChange {
                kind: ChangeKind::AddColumn,
                table: "posts".into(),
                schema: String::new(),
                column: Some(ColumnState {
                    name: "id".into(),
                    db_type: "BIGINT".into(),
                    nullable: false,
                    primary_key: true,
                    unique: false,
                    default: None,
                }),
                old_column: None,
            description: String::new(),
            },
            SchemaChange {
                kind: ChangeKind::AddColumn,
                table: "posts".into(),
                schema: String::new(),
                column: Some(ColumnState {
                    name: "title".into(),
                    db_type: "TEXT".into(),
                    nullable: false,
                    primary_key: false,
                    unique: false,
                    default: None,
                }),
                old_column: None,
            description: String::new(),
            },
        ];
        let sql = generate_ddl(&changes, Backend::PostgreSQL);
        assert_eq!(sql.len(), 1);
        assert!(sql[0].contains("CREATE TABLE"));
        assert!(sql[0].contains("BIGSERIAL"));
        assert!(sql[0].contains("PRIMARY KEY"));
    }

    #[test]
    fn test_ddl_add_column_existing_table() {
        let changes = vec![SchemaChange {
            kind: ChangeKind::AddColumn,
            table: "posts".into(),
            schema: String::new(),
            column: Some(ColumnState {
                name: "rating".into(),
                db_type: "INTEGER".into(),
                nullable: true,
                primary_key: false,
                unique: false,
                default: None,
            }),
            old_column: None,
        description: String::new(),
        }];
        let sql = generate_ddl(&changes, Backend::PostgreSQL);
        assert_eq!(sql.len(), 1);
        assert!(sql[0].contains("ALTER TABLE"));
        assert!(sql[0].contains("ADD COLUMN"));
    }

    #[test]
    fn test_ddl_alter_column_postgres() {
        let changes = vec![SchemaChange {
            kind: ChangeKind::AlterColumn,
            table: "posts".into(),
            schema: String::new(),
            column: Some(ColumnState {
                name: "title".into(),
                db_type: "VARCHAR".into(),
                nullable: false,
                primary_key: false,
                unique: false,
                default: None,
            }),
            old_column: Some(ColumnState {
                name: "title".into(),
                db_type: "TEXT".into(),
                nullable: true,
                primary_key: false,
                unique: false,
                default: None,
            }),
        description: String::new(),
        }];
        let sql = generate_ddl(&changes, Backend::PostgreSQL);
        assert_eq!(sql.len(), 2);
        assert!(sql[0].contains("ALTER TABLE"));
        assert!(sql[0].contains("TYPE VARCHAR"));
        assert!(sql[1].contains("SET NOT NULL"));
    }

    #[test]
    fn test_ddl_alter_column_mysql() {
        let changes = vec![SchemaChange {
            kind: ChangeKind::AlterColumn,
            table: "posts".into(),
            schema: String::new(),
            column: Some(ColumnState {
                name: "active".into(),
                db_type: "BOOLEAN".into(),
                nullable: false,
                primary_key: false,
                unique: false,
                default: None,
            }),
            old_column: Some(ColumnState {
                name: "active".into(),
                db_type: "INTEGER".into(),
                nullable: true,
                primary_key: false,
                unique: false,
                default: None,
            }),
        description: String::new(),
        }];
        let sql = generate_ddl(&changes, Backend::MySQL);
        assert_eq!(sql.len(), 1);
        assert!(sql[0].contains("MODIFY COLUMN"));
        assert!(sql[0].contains("TINYINT(1)"));
        assert!(sql[0].contains("NOT NULL"));
    }

    #[test]
    fn test_ddl_alter_column_sqlite_skipped() {
        let changes = vec![SchemaChange {
            kind: ChangeKind::AlterColumn,
            table: "posts".into(),
            schema: String::new(),
            column: Some(ColumnState {
                name: "title".into(),
                db_type: "VARCHAR".into(),
                nullable: false,
                primary_key: false,
                unique: false,
                default: None,
            }),
            old_column: Some(ColumnState {
                name: "title".into(),
                db_type: "TEXT".into(),
                nullable: true,
                primary_key: false,
                unique: false,
                default: None,
            }),
        description: String::new(),
        }];
        let sql = generate_ddl(&changes, Backend::SQLite);
        assert!(sql.is_empty(), "SQLite ALTER COLUMN should be no-op");
    }

    // ── get_text / get_int helpers ─────────────────────────

    fn make_row(pairs: &[(&str, SqlValue)]) -> DecodedRow {
        let mut values = Vec::new();
        let mut columns = Vec::new();
        for (k, v) in pairs {
            columns.push(k.to_string());
            values.push(v.clone());
        }
        DecodedRow {
            values,
            mapping: std::sync::Arc::new(ryx_backend::backends::RowMapping { columns }),
        }
    }

    #[test]
    fn test_get_text_found() {
        let row = make_row(&[("name", SqlValue::Text("hello".into()))]);
        assert_eq!(get_text(&row, "name"), Some("hello".into()));
    }

    #[test]
    fn test_get_text_missing() {
        let row = make_row(&[]);
        assert_eq!(get_text(&row, "name"), None);
    }

    #[test]
    fn test_get_int_found() {
        let row = make_row(&[("count", SqlValue::Int(42))]);
        assert_eq!(get_int(&row, "count"), Some(42));
    }

    #[test]
    fn test_get_int_null() {
        let row = make_row(&[("count", SqlValue::Null)]);
        assert_eq!(get_int(&row, "count"), None);
    }

    // ── Multi-schema tests ─────────────────────────────────

    fn table_in_schema(name: &str, schema: &str, cols: &[(&str, &str, bool)]) -> TableState {
        let mut t = table(name, cols);
        t.schema = schema.to_string();
        t
    }

    #[test]
    fn test_diff_create_schema_detected() {
        let current = SchemaState { tables: vec![] };
        let target = SchemaState {
            tables: vec![table_in_schema("posts", "tenant1", &[("id", "INTEGER", true)])],
        };
        let changes = diff_states(&current, &target);
        assert!(changes.iter().any(|c| c.kind == ChangeKind::CreateSchema),
            "Should detect CreateSchema for new schema 'tenant1'");
        assert!(changes.iter().any(|c| c.kind == ChangeKind::CreateTable),
            "Should also detect CreateTable for the new table");
        // CreateSchema should appear before CreateTable
        let idx_schema = changes.iter().position(|c| c.kind == ChangeKind::CreateSchema).unwrap();
        let idx_table = changes.iter().position(|c| c.kind == ChangeKind::CreateTable).unwrap();
        assert!(idx_schema < idx_table, "CreateSchema must precede CreateTable");
    }

    #[test]
    fn test_diff_same_table_different_schemas() {
        let current = SchemaState {
            tables: vec![table_in_schema("posts", "tenant1", &[("id", "INTEGER", true)])],
        };
        let target = SchemaState {
            tables: vec![
                table_in_schema("posts", "tenant1", &[("id", "INTEGER", true), ("title", "TEXT", false)]),
                table_in_schema("posts", "tenant2", &[("id", "INTEGER", true)]),
            ],
        };
        let changes = diff_states(&current, &target);
        assert!(changes.iter().any(|c| c.kind == ChangeKind::AddColumn && c.schema == "tenant1"),
            "Should add column to tenant1.posts");
        assert!(changes.iter().any(|c| c.kind == ChangeKind::CreateSchema && c.schema == "tenant2"),
            "Should CreateSchema for tenant2");
        assert!(changes.iter().any(|c| c.kind == ChangeKind::CreateTable && c.table == "posts" && c.schema == "tenant2"),
            "Should CreateTable for tenant2.posts");
    }

    #[test]
    fn test_diff_no_create_schema_for_empty_schema() {
        let current = SchemaState { tables: vec![] };
        let target = SchemaState {
            tables: vec![table("posts", &[("id", "INTEGER", true)])],
        };
        let changes = diff_states(&current, &target);
        // Empty schema tables should NOT trigger CreateSchema
        assert!(!changes.iter().any(|c| c.kind == ChangeKind::CreateSchema),
            "Empty schema should not produce CreateSchema");
        assert!(changes.iter().any(|c| c.kind == ChangeKind::CreateTable),
            "Should still CreateTable");
    }

    #[test]
    fn test_diff_schema_noop_when_identical() {
        let current = SchemaState {
            tables: vec![table_in_schema("posts", "tenant1", &[("id", "INTEGER", true)])],
        };
        let target = SchemaState {
            tables: vec![table_in_schema("posts", "tenant1", &[("id", "INTEGER", true)])],
        };
        let changes = diff_states(&current, &target);
        assert!(changes.is_empty(), "Identical schema states should produce no changes");
    }

    #[test]
    fn test_schema_change_carries_schema() {
        let current = SchemaState { tables: vec![] };
        let target = SchemaState {
            tables: vec![table_in_schema("posts", "tenant1", &[("id", "INTEGER", true)])],
        };
        let changes = diff_states(&current, &target);
        let create_table = changes.iter().find(|c| c.kind == ChangeKind::CreateTable).unwrap();
        assert_eq!(create_table.schema, "tenant1", "CreateTable change must carry schema");
    }

    #[test]
    fn test_diff_mixed_schemas_and_default() {
        // Tables both with and without schema in the same state
        let current = SchemaState { tables: vec![] };
        let target = SchemaState {
            tables: vec![
                table("users", &[("id", "INTEGER", true)]),                // no schema
                table_in_schema("posts", "blog", &[("id", "INTEGER", true)]),  // in 'blog'
            ],
        };
        let changes = diff_states(&current, &target);
        let create_schema_count = changes.iter().filter(|c| c.kind == ChangeKind::CreateSchema).count();
        assert_eq!(create_schema_count, 1, "Only one CreateSchema for 'blog'");
        assert!(!changes.iter().any(|c| c.kind == ChangeKind::CreateSchema && c.schema.is_empty()),
            "No CreateSchema for empty-schema tables");
        assert_eq!(changes.iter().filter(|c| c.kind == ChangeKind::CreateTable).count(), 2);
    }
}
