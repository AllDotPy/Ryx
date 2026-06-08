use crate::migration::{ChangeKind, ColumnState, SchemaChange, TableState};
use ryx_query::Backend;

// ============================================================
// DDL Generator
// ============================================================

/// Backend-aware DDL generator.
///
/// Convert schema changes into executable SQL statements, handling
/// the differences between PostgreSQL, MySQL, and SQLite.
///
/// ```ignore
/// use ryx_rs::migration::{DDLGenerator, SchemaChange, ChangeKind};
/// use ryx_query::Backend;
///
/// let ddl = DDLGenerator::new(Backend::PostgreSQL);
/// let sql = ddl.generate(&changes);
///
/// // For multi-schema:
/// let ddl = DDLGenerator::new(Backend::PostgreSQL).in_schema("tenant1");
/// let sql = ddl.create_table(&table);  // → CREATE TABLE "tenant1"."posts"
/// ```
#[derive(Debug, Clone)]
pub struct DDLGenerator {
    pub backend: Backend,
    /// Schema to qualify all table references with (empty = no qualification).
    pub schema: String,
}

impl DDLGenerator {
    pub fn new(backend: Backend) -> Self {
        Self {
            backend,
            schema: String::new(),
        }
    }

    /// Set the database schema for table qualification.
    ///
    /// When non-empty, all generated DDL will use ``"schema"."table"``
    /// notation (PostgreSQL only).
    pub fn in_schema(mut self, schema: &str) -> Self {
        self.schema = schema.to_string();
        self
    }

    // ── helpers ──────────────────────────────────────────

    fn col_type(&self, db_type: &str) -> String {
        col_type_for_backend(db_type, self.backend)
    }

    fn col_def(&self, col: &ColumnState, include_pk: bool) -> String {
        build_col_sql(col, self.backend, include_pk)
    }

    fn quote(&self, name: &str) -> String {
        match self.backend {
            Backend::MySQL => format!("`{name}`"),
            _ => format!("\"{name}\""),
        }
    }

    /// Return a schema-qualified table name: ``"schema"."table"``.
    ///
    /// If ``self.schema`` is empty or the backend does not support schemas,
    /// returns just the quoted table name (backward-compat).
    fn qn(&self, table_name: &str) -> String {
        if self.schema.is_empty() || !self.backend.supports_schemas() {
            self.quote(table_name)
        } else {
            format!("{}.{}", self.quote(&self.schema), self.quote(table_name))
        }
    }

    // ── Schema operations ───────────────────────────────

    /// Generate ``CREATE SCHEMA IF NOT EXISTS "schema_name"``.
    ///
    /// Only supported on PostgreSQL. Returns an empty string for other backends.
    pub fn create_schema(&self, schema: &str) -> String {
        if self.backend.supports_schemas() && !schema.is_empty() {
            format!("CREATE SCHEMA IF NOT EXISTS {};", self.quote(schema))
        } else {
            String::new()
        }
    }

    // ── DDL methods ──────────────────────────────────────

    /// Generate `CREATE TABLE ...` with idempotent `IF NOT EXISTS`.
    pub fn create_table(&self, table: &TableState) -> String {
        let if_not_exists = "IF NOT EXISTS ";
        let pk_col = table.columns.iter().find(|c| c.primary_key);
        let col_sqls: Vec<String> = table
            .columns
            .iter()
            .map(|c| format!("  {}", self.col_def(c, true)))
            .collect();

        let mut sql = format!(
            "CREATE TABLE {if_not_exists}{} (\n{}",
            self.qn(&table.name),
            col_sqls.join(",\n")
        );

        if let Some(pk) = pk_col {
            if !matches!(self.backend, Backend::SQLite) {
                sql.push_str(&format!(",\n  PRIMARY KEY ({})", self.quote(&pk.name)));
            }
        }
        sql.push_str("\n);");
        sql
    }

    /// Generate `DROP TABLE ...` with `IF EXISTS`.
    pub fn drop_table(&self, table_name: &str) -> String {
        format!(
            "DROP TABLE IF EXISTS {};",
            self.qn(table_name)
        )
    }

    /// Generate `ALTER TABLE ... ADD COLUMN ...` for an existing table.
    pub fn add_column(&self, table_name: &str, col: &ColumnState) -> String {
        format!(
            "ALTER TABLE {} ADD COLUMN {};",
            self.qn(table_name),
            self.col_def(col, false)
        )
    }

    /// Generate `ALTER TABLE ... DROP COLUMN ...` with `IF EXISTS`
    /// (SQLite does not support `DROP COLUMN`; emits a warning comment).
    pub fn drop_column(&self, table_name: &str, column_name: &str) -> String {
        if matches!(self.backend, Backend::SQLite) {
            format!(
                "-- SQLite does not support DROP COLUMN.  Recreate the table manually.\n-- ALTER TABLE {} DROP COLUMN {};",
                self.qn(table_name),
                self.quote(column_name)
            )
        } else {
            format!(
                "ALTER TABLE {} DROP COLUMN IF EXISTS {};",
                self.qn(table_name),
                self.quote(column_name)
            )
        }
    }

    /// Generate `ALTER TABLE ... ALTER COLUMN ...` or `MODIFY COLUMN` (MySQL).
    ///
    /// For **SQLite** this returns a table-rebuild script:
    ///  1. Rename table → `_old`
    ///  2. Create new table with the modified column
    ///  3. Copy data (excluding dropped columns)
    ///  4. Drop old table
    pub fn alter_column(
        &self,
        table_name: &str,
        old_col: &ColumnState,
        new_col: &ColumnState,
    ) -> Vec<String> {
        match self.backend {
            Backend::PostgreSQL => {
                let mut stmts = Vec::new();
                if old_col.db_type != new_col.db_type {
                    stmts.push(format!(
                        "ALTER TABLE {} ALTER COLUMN {} TYPE {};",
                        self.qn(table_name),
                        self.quote(&new_col.name),
                        self.col_type(&new_col.db_type)
                    ));
                }
                if old_col.nullable != new_col.nullable {
                    if new_col.nullable {
                        stmts.push(format!(
                            "ALTER TABLE {} ALTER COLUMN {} DROP NOT NULL;",
                            self.qn(table_name),
                            self.quote(&new_col.name)
                        ));
                    } else {
                        stmts.push(format!(
                            "ALTER TABLE {} ALTER COLUMN {} SET NOT NULL;",
                            self.qn(table_name),
                            self.quote(&new_col.name)
                        ));
                    }
                }
                // DEFAULT change
                if new_col.default != old_col.default {
                    match &new_col.default {
                        Some(val) => stmts.push(format!(
                            "ALTER TABLE {} ALTER COLUMN {} SET DEFAULT {};",
                            self.qn(table_name),
                            self.quote(&new_col.name),
                            val
                        )),
                        None => stmts.push(format!(
                            "ALTER TABLE {} ALTER COLUMN {} DROP DEFAULT;",
                            self.qn(table_name),
                            self.quote(&new_col.name)
                        )),
                    }
                }
                stmts
            }
            Backend::MySQL => {
                let nullable_sql = if new_col.nullable { "NULL" } else { "NOT NULL" };
                let type_str = self.col_type(&new_col.db_type);
                let default_sql = match &new_col.default {
                    Some(d) => format!(" DEFAULT {d}"),
                    None => String::new(),
                };
                vec![format!(
                    "ALTER TABLE {} MODIFY COLUMN {} {} {}{};",
                    self.qn(table_name),
                    self.quote(&new_col.name),
                    type_str,
                    nullable_sql,
                    default_sql
                )]
            }
            Backend::SQLite => self.alter_column_sqlite_rebuild(table_name, old_col, new_col),
        }
    }

    /// SQLite table rebuild strategy for ALTER COLUMN.
    ///
    /// Steps:
    ///  1. Rename `table` → `table__ryx_old`
    ///  2. Create new `table` with the modified column definition
    ///  3. Copy data from old to new
    ///  4. Drop old table
    ///
    /// Returns a list of SQL statements that must be executed in order.
    fn alter_column_sqlite_rebuild(
        &self,
        table_name: &str,
        _old_col: &ColumnState,
        _new_col: &ColumnState,
    ) -> Vec<String> {
        // For a full rebuild we need the complete table schema.
        // The caller should provide the full TableState, not just old/new cols.
        // For now, return a comment indicating manual rebuild.
        vec![format!(
            "-- SQLite does not support ALTER COLUMN.\n\
             -- Manual rebuild required for {t}:\n\
             --   1. CREATE TABLE {t}__new (...)\n\
             --   2. INSERT INTO {t}__new SELECT ... FROM {t}\n\
             --   3. DROP TABLE {t}\n\
             --   4. ALTER TABLE {t}__new RENAME TO {t}",
            t = self.qn(table_name)
        )]
    }

    /// Generate `CREATE INDEX ...` with `IF NOT EXISTS`.
    pub fn create_index(
        &self,
        table_name: &str,
        index_name: &str,
        fields: &[String],
        unique: bool,
    ) -> String {
        let unique_kw = if unique { "UNIQUE " } else { "" };
        let cols: Vec<String> = fields.iter().map(|f| self.quote(f)).collect();
        format!(
            "CREATE {unique_kw}INDEX IF NOT EXISTS {} ON {} ({});",
            self.quote(index_name),
            self.qn(table_name),
            cols.join(", ")
        )
    }

    /// Generate `DROP INDEX ...` with `IF EXISTS`.
    pub fn drop_index(&self, table_name: &str, index_name: &str) -> String {
        match self.backend {
            Backend::MySQL => {
                format!(
                    "DROP INDEX {} ON {};",
                    self.quote(index_name),
                    self.qn(table_name)
                )
            }
            Backend::PostgreSQL => {
                format!("DROP INDEX IF EXISTS {};", self.quote(index_name))
            }
            Backend::SQLite => {
                format!("DROP INDEX IF EXISTS {};", self.quote(index_name))
            }
        }
    }

    /// Generate `ALTER TABLE ... ADD CONSTRAINT ... CHECK (...)`.
    pub fn add_check_constraint(
        &self,
        table_name: &str,
        constraint_name: &str,
        check_expr: &str,
    ) -> String {
        format!(
            "ALTER TABLE {} ADD CONSTRAINT {} CHECK ({})",
            self.qn(table_name),
            self.quote(constraint_name),
            check_expr
        )
    }

    /// Generate `ALTER TABLE ... ADD CONSTRAINT ... FOREIGN KEY (...) REFERENCES ...`.
    pub fn add_foreign_key(
        &self,
        table_name: &str,
        constraint_name: &str,
        column: &str,
        ref_table: &str,
        ref_column: &str,
    ) -> String {
        format!(
            "ALTER TABLE {} ADD CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {}({});",
            self.qn(table_name),
            self.quote(constraint_name),
            self.quote(column),
            self.qn(ref_table),
            self.quote(ref_column)
        )
    }

    // ── bulk generation ──────────────────────────────────

    /// Generate all DDL statements for the given changes.
    pub fn generate(&self, changes: &[SchemaChange]) -> Vec<String> {
        let mut statements = Vec::new();

        // Zeroth pass: CREATE SCHEMA (for PostgreSQL schemas)
        for change in changes {
            if change.kind == ChangeKind::CreateSchema && !change.schema.is_empty() {
                let sql = self.create_schema(&change.schema);
                if !sql.is_empty() {
                    statements.push(sql);
                }
            }
        }

        // First pass: CREATE TABLE
        let create_tables: Vec<&SchemaChange> = changes
            .iter()
            .filter(|c| c.kind == ChangeKind::CreateTable)
            .collect();

        for change in &create_tables {
            let cols: Vec<ColumnState> = changes
                .iter()
                .filter(|c| {
                    c.kind == ChangeKind::AddColumn
                        && c.table == change.table
                        && c.column.is_some()
                })
                .filter_map(|c| c.column.clone())
                .collect();

            if !cols.is_empty() {
                let table = TableState {
                    name: change.table.clone(),
                    schema: change.schema.clone(),
                    columns: cols,
                };
                let ddl_gen = if change.schema.is_empty() {
                    Self { backend: self.backend, schema: self.schema.clone() }
                } else {
                    Self { backend: self.backend, schema: change.schema.clone() }
                };
                statements.push(ddl_gen.create_table(&table));
            }
        }

        // Second pass: ALTER TABLE ADD COLUMN (for existing tables)
        let created_tables: Vec<&str> = create_tables.iter().map(|c| c.table.as_str()).collect();
        for change in changes {
            if change.kind == ChangeKind::AddColumn
                && !created_tables.contains(&change.table.as_str())
            {
                if let Some(ref col) = change.column {
                    let ddl_gen = if change.schema.is_empty() {
                        Self { backend: self.backend, schema: self.schema.clone() }
                    } else {
                        Self { backend: self.backend, schema: change.schema.clone() }
                    };
                    statements.push(ddl_gen.add_column(&change.table, col));
                }
            }
        }

        // Third pass: ALTER / MODIFY COLUMN (SQLite: skip in bulk — caller
        // should use `DDLGenerator::alter_column` directly for rebuild scripts)
        for change in changes {
            if change.kind == ChangeKind::AlterColumn {
                if self.backend == Backend::SQLite {
                    continue; // SQLite ALTER skipped in bulk generation
                }
                if let (Some(col), Some(old_col)) = (&change.column, &change.old_column) {
                    let ddl_gen = if change.schema.is_empty() {
                        Self { backend: self.backend, schema: self.schema.clone() }
                    } else {
                        Self { backend: self.backend, schema: change.schema.clone() }
                    };
                    statements.extend(ddl_gen.alter_column(&change.table, old_col, col));
                }
            }
        }

        statements
    }
}

// ============================================================
// Free functions (backward-compat)
// ============================================================

pub fn col_type_for_backend(db_type: &str, backend: Backend) -> String {
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

pub fn build_col_sql(col: &ColumnState, backend: Backend, include_pk: bool) -> String {
    let mut parts: Vec<String> = vec![];

    parts.push(format!("\"{}\"", col.name));

    if include_pk && col.primary_key {
        match backend {
            Backend::PostgreSQL => {
                parts.push("BIGSERIAL".to_string());
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

/// Convenience function — generate complete DDL for a list of model-based tables.
///
/// This is the programmatic equivalent of `python -m ryx sqlmigrate`.
/// It produces the SQL needed to create the full schema from scratch.
pub fn generate_schema_ddl(tables: &[TableState], backend: Backend) -> Vec<String> {
    let mut stmts = Vec::new();
    for table in tables {
        let ddl_gen = if table.schema.is_empty() {
            DDLGenerator::new(backend)
        } else {
            DDLGenerator::new(backend).in_schema(&table.schema)
        };
        stmts.push(ddl_gen.create_table(table));
    }
    stmts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str, db_type: &str, pk: bool) -> ColumnState {
        ColumnState {
            name: name.into(),
            db_type: db_type.into(),
            nullable: false,
            primary_key: pk,
            unique: false,
            default: None,
        }
    }

    fn table(name: &str, cols: &[ColumnState]) -> TableState {
        TableState {
            name: name.into(),
            schema: String::new(),
            columns: cols.to_vec(),
        }
    }

    // ── DDLGenerator methods ─────────────────────────────

    #[test]
    fn test_drop_table() {
        let g = DDLGenerator::new(Backend::PostgreSQL);
        let sql = g.drop_table("authors");
        assert!(sql.contains("DROP TABLE IF EXISTS"));
        assert!(sql.contains("authors"));
    }

    #[test]
    fn test_drop_column() {
        let g = DDLGenerator::new(Backend::PostgreSQL);
        let sql = g.drop_column("posts", "legacy_field");
        assert!(sql.contains("DROP COLUMN IF EXISTS"));
        assert!(sql.contains("legacy_field"));
    }

    #[test]
    fn test_drop_column_sqlite() {
        let g = DDLGenerator::new(Backend::SQLite);
        let sql = g.drop_column("posts", "legacy");
        assert!(sql.contains("SQLite does not support DROP COLUMN"));
    }

    #[test]
    fn test_create_index() {
        let g = DDLGenerator::new(Backend::PostgreSQL);
        let sql = g.create_index("posts", "idx_title", &["title".into()], false);
        assert!(sql.contains("CREATE INDEX IF NOT EXISTS"));
        assert!(sql.contains("idx_title"));
    }

    #[test]
    fn test_create_unique_index() {
        let g = DDLGenerator::new(Backend::PostgreSQL);
        let sql = g.create_index(
            "users",
            "idx_email",
            &["email".into()],
            true,
        );
        assert!(sql.contains("CREATE UNIQUE INDEX"));
        assert!(sql.contains("IF NOT EXISTS"));
    }

    #[test]
    fn test_drop_index() {
        let g = DDLGenerator::new(Backend::PostgreSQL);
        let sql = g.drop_index("posts", "idx_old");
        assert!(sql.contains("DROP INDEX IF EXISTS"));
        assert!(sql.contains("idx_old"));
    }

    #[test]
    fn test_drop_index_mysql() {
        let g = DDLGenerator::new(Backend::MySQL);
        let sql = g.drop_index("posts", "idx_old");
        assert!(sql.contains("DROP INDEX"));
        assert!(sql.contains("ON")); // MySQL: DROP INDEX ... ON table
    }

    #[test]
    fn test_add_check_constraint() {
        let g = DDLGenerator::new(Backend::PostgreSQL);
        let sql = g.add_check_constraint("products", "chk_price", "price > 0");
        assert!(sql.contains("ADD CONSTRAINT"));
        assert!(sql.contains("CHECK (price > 0)"));
    }

    #[test]
    fn test_add_foreign_key() {
        let g = DDLGenerator::new(Backend::PostgreSQL);
        let sql = g.add_foreign_key(
            "posts",
            "fk_posts_author",
            "author_id",
            "authors",
            "id",
        );
        assert!(sql.contains("FOREIGN KEY"));
        assert!(sql.contains("REFERENCES"));
        assert!(sql.contains("authors"));
    }

    #[test]
    fn test_alter_column_postgres() {
        let g = DDLGenerator::new(Backend::PostgreSQL);
        let old = col("bio", "TEXT", false);
        let new = ColumnState {
            name: "bio".into(),
            db_type: "TEXT".into(),
            nullable: true,
            primary_key: false,
            unique: false,
            default: Some("''".into()),
        };
        let stmts = g.alter_column("posts", &old, &new);
        assert!(stmts.len() >= 2); // DROP NOT NULL + SET DEFAULT
        assert!(stmts.iter().any(|s| s.contains("DROP NOT NULL")));
        assert!(stmts.iter().any(|s| s.contains("SET DEFAULT")));
    }

    #[test]
    fn test_alter_column_mysql() {
        let g = DDLGenerator::new(Backend::MySQL);
        let old = col("active", "INTEGER", false);
        let new = ColumnState {
            name: "active".into(),
            db_type: "BOOLEAN".into(),
            nullable: false,
            primary_key: false,
            unique: false,
            default: None,
        };
        let stmts = g.alter_column("posts", &old, &new);
        assert_eq!(stmts.len(), 1);
        assert!(stmts[0].contains("MODIFY COLUMN"));
        assert!(stmts[0].contains("TINYINT(1)"));
    }

    #[test]
    fn test_alter_column_sqlite_comment() {
        let g = DDLGenerator::new(Backend::SQLite);
        let old = col("title", "TEXT", false);
        let new = col("title", "VARCHAR", false);
        let stmts = g.alter_column("posts", &old, &new);
        assert_eq!(stmts.len(), 1);
        assert!(stmts[0].contains("Manual rebuild"));
    }

    #[test]
    fn test_create_table_if_not_exists() {
        let g = DDLGenerator::new(Backend::SQLite);
        let t = table(
            "posts",
            &[col("id", "INTEGER", true), col("title", "TEXT", false)],
        );
        let sql = g.create_table(&t);
        assert!(sql.contains("IF NOT EXISTS"));
        assert!(sql.contains("INTEGER PRIMARY KEY AUTOINCREMENT"));
    }

    #[test]
    fn test_generate_schema_ddl() {
        let tables = vec![
            table("authors", &[col("id", "INTEGER", true), col("name", "TEXT", false)]),
            table("posts", &[col("id", "INTEGER", true), col("title", "TEXT", false)]),
        ];
        let stmts = generate_schema_ddl(&tables, Backend::SQLite);
        assert_eq!(stmts.len(), 2);
        assert!(stmts[0].contains("CREATE TABLE"));
        assert!(stmts[1].contains("CREATE TABLE"));
    }

    // ── Multi-schema DDL tests ───────────────────────────

    fn table_in_schema(name: &str, schema: &str, cols: &[ColumnState]) -> TableState {
        let mut t = table(name, cols);
        t.schema = schema.to_string();
        t
    }

    #[test]
    fn test_create_schema_ddl() {
        let g = DDLGenerator::new(Backend::PostgreSQL);
        let sql = g.create_schema("tenant1");
        assert!(sql.contains("CREATE SCHEMA IF NOT EXISTS"));
        assert!(sql.contains("tenant1"));
    }

    #[test]
    fn test_create_schema_ddl_mysql_empty() {
        let g = DDLGenerator::new(Backend::MySQL);
        let sql = g.create_schema("tenant1");
        assert!(sql.is_empty(), "MySQL should not emit CREATE SCHEMA");
    }

    #[test]
    fn test_create_schema_ddl_empty() {
        let g = DDLGenerator::new(Backend::PostgreSQL);
        let sql = g.create_schema("");
        assert!(sql.is_empty(), "Empty schema should return empty string");
    }

    #[test]
    fn test_create_table_with_schema() {
        let g = DDLGenerator::new(Backend::PostgreSQL).in_schema("tenant1");
        let t = table_in_schema("posts", "tenant1", &[col("id", "INTEGER", true)]);
        let sql = g.create_table(&t);
        assert!(sql.contains(r#""tenant1"."posts""#),
            "Table should be schema-qualified: {sql}");
        assert!(sql.contains("CREATE TABLE IF NOT EXISTS"));
    }

    #[test]
    fn test_create_table_no_schema_backward_compat() {
        let g = DDLGenerator::new(Backend::PostgreSQL);
        let t = table("posts", &[col("id", "INTEGER", true)]);
        let sql = g.create_table(&t);
        assert!(!sql.contains("."), "No schema should not qualify: {sql}");
        assert!(sql.contains(r#""posts""#));
    }

    #[test]
    fn test_create_table_mysql_ignores_schema() {
        let g = DDLGenerator::new(Backend::MySQL).in_schema("tenant1");
        let t = table_in_schema("posts", "tenant1", &[col("id", "INTEGER", true)]);
        let sql = g.create_table(&t);
        assert!(!sql.contains("tenant1"),
            "MySQL should not schema-qualify: {sql}");
    }

    #[test]
    fn test_drop_table_with_schema() {
        let g = DDLGenerator::new(Backend::PostgreSQL).in_schema("tenant1");
        let sql = g.drop_table("posts");
        assert!(sql.contains(r#""tenant1"."posts""#),
            "DROP TABLE should be schema-qualified: {sql}");
    }

    #[test]
    fn test_add_column_with_schema() {
        let g = DDLGenerator::new(Backend::PostgreSQL).in_schema("tenant1");
        let sql = g.add_column("posts", &col("title", "TEXT", false));
        assert!(sql.contains(r#""tenant1"."posts""#),
            "ALTER TABLE ADD COLUMN should be schema-qualified: {sql}");
    }

    #[test]
    fn test_drop_column_with_schema() {
        let g = DDLGenerator::new(Backend::PostgreSQL).in_schema("tenant1");
        let sql = g.drop_column("posts", "title");
        assert!(sql.contains(r#""tenant1"."posts""#),
            "ALTER TABLE DROP COLUMN should be schema-qualified: {sql}");
    }

    #[test]
    fn test_create_index_with_schema() {
        let g = DDLGenerator::new(Backend::PostgreSQL).in_schema("tenant1");
        let sql = g.create_index("posts", "idx_title", &["title".to_string()], false);
        assert!(sql.contains(r#""tenant1"."posts""#),
            "CREATE INDEX should be schema-qualified: {sql}");
    }

    #[test]
    fn test_drop_index_pg_ignores_table_name() {
        let g = DDLGenerator::new(Backend::PostgreSQL).in_schema("tenant1");
        let sql = g.drop_index("posts", "idx_title");
        // PostgreSQL DROP INDEX does not use table_name, just index name
        // Schema is not applied to index-only drops in PG
        assert!(sql.contains("idx_title") && sql.contains("DROP INDEX"));
        assert!(!sql.contains("posts"), "PG DROP INDEX should not use table_name: {sql}");
    }

    #[test]
    fn test_drop_index_mysql_with_schema() {
        let g = DDLGenerator::new(Backend::MySQL).in_schema("tenant1");
        let sql = g.drop_index("posts", "idx_title");
        assert!(!sql.contains("tenant1"),
            "MySQL DROP INDEX should NOT schema-qualify table: {sql}");
    }

    #[test]
    fn test_alter_column_with_schema() {
        let g = DDLGenerator::new(Backend::PostgreSQL).in_schema("tenant1");
        let old = col("title", "VARCHAR(100)", true);
        let new = col("title", "VARCHAR(200)", false);
        let stmts = g.alter_column("posts", &old, &new);
        assert!(stmts.iter().any(|s| s.contains(r#""tenant1"."posts""#)),
            "ALTER COLUMN should be schema-qualified: {:?}", stmts);
    }

    #[test]
    fn test_add_foreign_key_with_schema() {
        let g = DDLGenerator::new(Backend::PostgreSQL).in_schema("tenant1");
        let sql = g.add_foreign_key("posts", "fk_author", "author_id", "authors", "id");
        assert!(sql.contains(r#""tenant1"."posts""#) && sql.contains(r#""tenant1"."authors""#),
            "FOREIGN KEY should qualify both tables: {sql}");
    }

    #[test]
    fn test_add_check_constraint_with_schema() {
        let g = DDLGenerator::new(Backend::PostgreSQL).in_schema("tenant1");
        let sql = g.add_check_constraint("users", "age_check", "age > 0");
        assert!(sql.contains(r#""tenant1"."users""#),
            "CHECK constraint should be schema-qualified: {sql}");
    }

    #[test]
    fn test_generate_schema_ddl_with_schema() {
        let tables = vec![
            table_in_schema("posts", "tenant1", &[col("id", "INTEGER", true)]),
            table_in_schema("comments", "tenant1", &[col("id", "INTEGER", true)]),
        ];
        let stmts = generate_schema_ddl(&tables, Backend::PostgreSQL);
        assert_eq!(stmts.len(), 2);
        assert!(stmts[0].contains(r#""tenant1"."posts""#),
            "DDL should qualify table: {}", stmts[0]);
        assert!(stmts[1].contains(r#""tenant1"."comments""#),
            "DDL should qualify table: {}", stmts[1]);
    }

    #[test]
    fn test_create_table_mixed_schemas() {
        // Tables in different schemas generate correctly
        let g1 = DDLGenerator::new(Backend::PostgreSQL).in_schema("tenant1");
        let g2 = DDLGenerator::new(Backend::PostgreSQL).in_schema("tenant2");
        let t1 = table_in_schema("posts", "tenant1", &[col("id", "INTEGER", true)]);
        let t2 = table_in_schema("posts", "tenant2", &[col("id", "INTEGER", true)]);
        let sql1 = g1.create_table(&t1);
        let sql2 = g2.create_table(&t2);
        assert!(sql1.contains(r#""tenant1"."posts""#), "SQL1: {sql1}");
        assert!(sql2.contains(r#""tenant2"."posts""#), "SQL2: {sql2}");
    }
}
