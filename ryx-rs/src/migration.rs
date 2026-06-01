use ryx_backend::backends::{DecodedRow, RyxBackend};
use ryx_common::RyxResult;
use ryx_query::Backend;

use crate::model::{FieldMeta, Model};

// ============================================================
// State types
// ============================================================

/// A snapshot of a single database column, as seen in the live DB
/// or as declared by a model.
#[derive(Debug, Clone, PartialEq)]
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
#[derive(Debug, Clone, PartialEq)]
pub struct TableState {
    pub name: String,
    pub columns: Vec<ColumnState>,
}

/// A full schema as known by the database or by the model declarations.
#[derive(Debug, Clone, PartialEq)]
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
}

/// A single schema change operation.
#[derive(Debug, Clone, PartialEq)]
pub struct SchemaChange {
    pub kind: ChangeKind,
    pub table: String,
    pub column: Option<ColumnState>,
    pub old_column: Option<ColumnState>,
}

/// Compare two schema states and return the list of changes needed to
/// go from `current` to `target`.
pub fn diff_states(current: &SchemaState, target: &SchemaState) -> Vec<SchemaChange> {
    let mut changes = Vec::new();

    // Tables in target but not in current → CREATE
    for table in &target.tables {
        let current_table = current.tables.iter().find(|t| t.name == table.name);
        if current_table.is_none() {
            changes.push(SchemaChange {
                kind: ChangeKind::CreateTable,
                table: table.name.clone(),
                column: None,
                old_column: None,
            });
        }
    }

    // Columns of newly created tables → also emit AddColumn (so generate_ddl can use them)
    for table in &target.tables {
        if current.tables.iter().any(|t| t.name == table.name) {
            continue;
        }
        for col in &table.columns {
            changes.push(SchemaChange {
                kind: ChangeKind::AddColumn,
                table: table.name.clone(),
                column: Some(col.clone()),
                old_column: None,
            });
        }
    }

    // Columns in target but not in current → ADD COLUMN
    for table in &target.tables {
        if let Some(current_table) = current.tables.iter().find(|t| t.name == table.name) {
            let current_names: Vec<&str> =
                current_table.columns.iter().map(|c| c.name.as_str()).collect();
            for col in &table.columns {
                if !current_names.contains(&col.name.as_str()) {
                    changes.push(SchemaChange {
                        kind: ChangeKind::AddColumn,
                        table: table.name.clone(),
                        column: Some(col.clone()),
                        old_column: None,
                    });
                }
            }
        }
    }

    // Columns in both but different → ALTER COLUMN
    for table in &target.tables {
        if let Some(current_table) = current.tables.iter().find(|t| t.name == table.name) {
            for col in &table.columns {
                if let Some(current_col) =
                    current_table.columns.iter().find(|c| c.name == col.name)
                {
                    if col != current_col {
                        changes.push(SchemaChange {
                            kind: ChangeKind::AlterColumn,
                            table: table.name.clone(),
                            column: Some(col.clone()),
                            old_column: Some(current_col.clone()),
                        });
                    }
                }
            }
        }
    }

    changes
}

// ============================================================
// DDL Generator
// ============================================================

fn col_type_for_backend(db_type: &str, backend: Backend) -> String {
    match backend {
        Backend::PostgreSQL => match db_type {
            "BOOLEAN" => "BOOLEAN",
            "INTEGER" => "INTEGER",
            "BIGINT" => "BIGINT",
            "DOUBLE PRECISION" => "DOUBLE PRECISION",
            "REAL" => "REAL",
            "TEXT" => "TEXT",
            "TIMESTAMP" => "TIMESTAMP",
            "DATE" => "DATE",
            "TIME" => "TIME",
            "UUID" => "UUID",
            "JSONB" => "JSONB",
            other => other,
        }
        .to_string(),
        Backend::MySQL => match db_type {
            "BOOLEAN" => "TINYINT(1)",
            "INTEGER" => "INT",
            "BIGINT" => "BIGINT",
            "DOUBLE PRECISION" => "DOUBLE",
            "REAL" => "FLOAT",
            "TEXT" => "TEXT",
            "TIMESTAMP" => "DATETIME",
            "UUID" => "CHAR(36)",
            "JSONB" => "JSON",
            other => other,
        }
        .to_string(),
        Backend::SQLite => match db_type {
            "BOOLEAN" | "INTEGER" | "BIGINT" => "INTEGER",
            "DOUBLE PRECISION" | "REAL" => "REAL",
            "UUID" | "JSONB" => "TEXT",
            other => other,
        }
        .to_string(),
    }
}

fn build_col_sql(col: &ColumnState, backend: Backend, include_pk: bool) -> String {
    let mut parts: Vec<String> = vec![];
    parts.push(format!("\"{}\"", col.name));

    if include_pk && col.primary_key {
        match backend {
            Backend::PostgreSQL => {
                parts.push("BIGSERIAL".to_string());
                // PK implies NOT NULL
            }
            Backend::MySQL => {
                parts.push("INT AUTO_INCREMENT".to_string());
            }
            Backend::SQLite => {
                parts.push("INTEGER PRIMARY KEY AUTOINCREMENT".to_string());
                return parts.join(" ");
            }
        }
    } else {
        parts.push(col_type_for_backend(&col.db_type, backend));
        if !col.nullable {
            parts.push("NOT NULL".to_string());
        }
        if col.unique {
            parts.push("UNIQUE".to_string());
        }
        if let Some(ref def) = col.default {
            parts.push(format!("DEFAULT {def}"));
        }
    }

    parts.join(" ")
}

/// Generate DDL statements for the given changes on the specified backend.
pub fn generate_ddl(changes: &[SchemaChange], backend: Backend) -> Vec<String> {
    let mut statements = Vec::new();

    // First pass: CREATE TABLE statements
    let create_tables: Vec<&SchemaChange> = changes
        .iter()
        .filter(|c| c.kind == ChangeKind::CreateTable)
        .collect();

    for change in &create_tables {
        // Find all AddColumn for this table (they come right after the CreateTable)
        let cols: Vec<ColumnState> = changes
            .iter()
            .filter(|c| {
                c.kind == ChangeKind::AddColumn
                    && c.table == change.table
                    && c.column.is_some()
            })
            .filter_map(|c| c.column.clone())
            .collect();

        if cols.is_empty() {
            continue;
        }

        let pk_col = cols.iter().find(|c| c.primary_key);
        let col_sqls: Vec<String> = cols
            .iter()
            .map(|c| format!("  {}", build_col_sql(c, backend, true)))
            .collect();
        let mut sql = format!("CREATE TABLE \"{}\" (\n{}", change.table, col_sqls.join(",\n"));
        if let Some(pk) = pk_col {
            if !matches!(backend, Backend::SQLite) {
                sql.push_str(&format!(",\n  PRIMARY KEY (\"{}\")", pk.name));
            }
        }
        sql.push_str("\n);");
        statements.push(sql);
    }

    // Second pass: ALTER TABLE for columns added to existing tables
    let created_tables: Vec<&str> = create_tables.iter().map(|c| c.table.as_str()).collect();
    for change in changes {
        if change.kind == ChangeKind::AddColumn {
            // Skip if part of a CREATE TABLE
            if created_tables.contains(&change.table.as_str()) {
                continue;
            }
            if let Some(ref col) = change.column {
                let col_sql = build_col_sql(col, backend, false);
                statements.push(format!(
                    "ALTER TABLE \"{}\" ADD COLUMN {};",
                    change.table, col_sql
                ));
            }
        }
    }

    // Third pass: ALTER COLUMN
    for change in changes {
        if change.kind == ChangeKind::AlterColumn {
            if let Some(ref col) = change.column {
                let type_str = col_type_for_backend(&col.db_type, backend);
                match backend {
                    Backend::PostgreSQL => {
                        statements.push(format!(
                            "ALTER TABLE \"{}\" ALTER COLUMN \"{}\" TYPE {};",
                            change.table, col.name, type_str
                        ));
                        if col.nullable {
                            statements.push(format!(
                                "ALTER TABLE \"{}\" ALTER COLUMN \"{}\" DROP NOT NULL;",
                                change.table, col.name
                            ));
                        } else {
                            statements.push(format!(
                                "ALTER TABLE \"{}\" ALTER COLUMN \"{}\" SET NOT NULL;",
                                change.table, col.name
                            ));
                        }
                    }
                    Backend::MySQL => {
                        let nullable_sql = if col.nullable { "NULL" } else { "NOT NULL" };
                        statements.push(format!(
                            "ALTER TABLE \"{}\" MODIFY COLUMN \"{}\" {} {};",
                            change.table, col.name, type_str, nullable_sql
                        ));
                    }
                    Backend::SQLite => {
                        // SQLite doesn't support ALTER COLUMN — manual rebuild required
                    }
                }
            }
        }
    }

    statements
}

// ============================================================
// Introspection
// ============================================================

const MIGRATIONS_TABLE: &str = "ryx_migrations";

/// Introspect the live database and return its current `SchemaState`.
pub async fn introspect_schema(
    backend: &dyn RyxBackend,
    backend_type: Backend,
) -> RyxResult<SchemaState> {
    match backend_type {
        Backend::PostgreSQL => introspect_schema_postgres(backend).await,
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
        tables.push(TableState { name: table_name, columns });
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

async fn introspect_schema_postgres(backend: &dyn RyxBackend) -> RyxResult<SchemaState> {
    let table_rows = backend
        .fetch_raw(
            "SELECT table_name FROM information_schema.tables \
             WHERE table_schema = 'public' AND table_type = 'BASE TABLE' \
             AND table_name != 'ryx_migrations'"
                .to_string(),
            None,
        )
        .await?;

    let mut tables = Vec::new();
    for row in &table_rows {
        let table_name = get_text(row, "table_name").unwrap_or_default();
        let columns = introspect_columns_postgres(backend, &table_name).await?;
        tables.push(TableState { name: table_name, columns });
    }
    Ok(SchemaState { tables })
}

async fn introspect_columns_postgres(
    backend: &dyn RyxBackend,
    table: &str,
) -> RyxResult<Vec<ColumnState>> {
    let sql = format!(
        "SELECT column_name, data_type, is_nullable, column_default \
         FROM information_schema.columns \
         WHERE table_schema = 'public' AND table_name = '{table}' \
         ORDER BY ordinal_position"
    );
    let rows = backend.fetch_raw(sql, None).await?;

    // Get primary key columns
    let pk_cols = get_constraint_columns_postgres(backend, table, "PRIMARY KEY").await?;
    let unique_cols = get_constraint_columns_postgres(backend, table, "UNIQUE").await?;

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
    constraint_type: &str,
) -> RyxResult<Vec<String>> {
    let sql = format!(
        "SELECT kcu.column_name \
         FROM information_schema.table_constraints tc \
         JOIN information_schema.key_column_usage kcu \
           ON tc.constraint_name = kcu.constraint_name \
          AND tc.table_schema = kcu.table_schema \
          AND tc.table_name = kcu.table_name \
         WHERE tc.table_schema = 'public' \
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
        tables.push(TableState { name: table_name, columns });
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

fn get_text(row: &DecodedRow, key: &str) -> Option<String> {
    row.get(key).and_then(|v| match v {
        ryx_query::ast::SqlValue::Text(s) => Some(s.clone()),
        _ => None,
    })
}

fn get_int(row: &DecodedRow, key: &str) -> Option<i64> {
    row.get(key).and_then(|v| match v {
        ryx_query::ast::SqlValue::Int(n) => Some(*n),
        _ => None,
    })
}

// ============================================================
// Migration Runner
// ============================================================

type ModelInfoFn = fn() -> TableState;

/// Apply migrations — introspect, diff, and execute DDL.
///
/// ```ignore
/// use ryx_rs::migration::MigrationRunner;
///
/// MigrationRunner::new()
///     .model::<Post>()
///     .model::<Author>()
///     .run().await?;
///
/// // Multi-db: target a specific database alias
/// MigrationRunner::new()
///     .db("replica")
///     .model::<Post>()
///     .run().await?;
/// ```
pub struct MigrationRunner {
    models: Vec<ModelInfoFn>,
    db_alias: Option<String>,
}

impl MigrationRunner {
    pub fn new() -> Self {
        Self {
            models: vec![],
            db_alias: None,
        }
    }

    /// Target a specific database alias (defaults to `"default"`).
    pub fn db(mut self, alias: &str) -> Self {
        self.db_alias = Some(alias.to_string());
        self
    }

    /// Register a model for migration.
    pub fn model<M: Model>(mut self) -> Self {
        self.models.push(|| {
            let meta = M::field_meta();
            let columns: Vec<ColumnState> = meta.iter().map(|m| m.into()).collect();
            TableState {
                name: M::table_name().to_string(),
                columns,
            }
        });
        self
    }

    fn build_target(&self) -> SchemaState {
        let mut tables = Vec::new();
        for info_fn in &self.models {
            tables.push(info_fn());
        }
        SchemaState { tables }
    }

    /// Diff models against the live DB and apply changes.
    pub async fn run(self) -> RyxResult<()> {
        let alias = self.db_alias.as_deref();
        let pool = ryx_backend::pool::get(alias)?;
        let target = self.build_target();

        // Ensure migrations tracking table
        let create_tracking = format!(
            "CREATE TABLE IF NOT EXISTS \"{}\" (\
             id INTEGER PRIMARY KEY,\
             name TEXT NOT NULL UNIQUE,\
             applied_at TEXT NOT NULL\
             )",
            MIGRATIONS_TABLE
        );
        let _ = pool.fetch_raw(create_tracking, None).await?;

        let backend_type = ryx_backend::pool::get_backend(alias)?;
        let current = introspect_schema(pool.as_ref(), backend_type).await?;
        let changes = diff_states(&current, &target);

        if changes.is_empty() {
            return Ok(());
        }

        let ddl = generate_ddl(&changes, backend_type);

        for stmt in &ddl {
            pool.fetch_raw(stmt.clone(), None).await?;
        }

        Ok(())
    }

    /// Preview the DDL that would be applied (dry-run).
    pub async fn plan(self) -> RyxResult<Vec<String>> {
        let alias = self.db_alias.as_deref();
        let pool = ryx_backend::pool::get(alias)?;
        let backend_type = ryx_backend::pool::get_backend(alias)?;
        let current = introspect_schema(pool.as_ref(), backend_type).await?;
        let target = self.build_target();
        let changes = diff_states(&current, &target);
        Ok(generate_ddl(&changes, backend_type))
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
                column: None,
                old_column: None,
            },
            SchemaChange {
                kind: ChangeKind::AddColumn,
                table: "posts".into(),
                column: Some(ColumnState {
                    name: "id".into(),
                    db_type: "INTEGER".into(),
                    nullable: false,
                    primary_key: true,
                    unique: false,
                    default: None,
                }),
                old_column: None,
            },
            SchemaChange {
                kind: ChangeKind::AddColumn,
                table: "posts".into(),
                column: Some(ColumnState {
                    name: "title".into(),
                    db_type: "TEXT".into(),
                    nullable: false,
                    primary_key: false,
                    unique: false,
                    default: None,
                }),
                old_column: None,
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
                column: None,
                old_column: None,
            },
            SchemaChange {
                kind: ChangeKind::AddColumn,
                table: "posts".into(),
                column: Some(ColumnState {
                    name: "id".into(),
                    db_type: "BIGINT".into(),
                    nullable: false,
                    primary_key: true,
                    unique: false,
                    default: None,
                }),
                old_column: None,
            },
            SchemaChange {
                kind: ChangeKind::AddColumn,
                table: "posts".into(),
                column: Some(ColumnState {
                    name: "title".into(),
                    db_type: "TEXT".into(),
                    nullable: false,
                    primary_key: false,
                    unique: false,
                    default: None,
                }),
                old_column: None,
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
            column: Some(ColumnState {
                name: "rating".into(),
                db_type: "INTEGER".into(),
                nullable: true,
                primary_key: false,
                unique: false,
                default: None,
            }),
            old_column: None,
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
}
