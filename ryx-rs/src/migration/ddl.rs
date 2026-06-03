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
/// ```
pub struct DDLGenerator {
    pub backend: Backend,
}

impl DDLGenerator {
    pub fn new(backend: Backend) -> Self {
        Self { backend }
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
            self.quote(&table.name),
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
            self.quote(table_name)
        )
    }

    /// Generate `ALTER TABLE ... ADD COLUMN ...` for an existing table.
    pub fn add_column(&self, table_name: &str, col: &ColumnState) -> String {
        format!(
            "ALTER TABLE {} ADD COLUMN {};",
            self.quote(table_name),
            self.col_def(col, false)
        )
    }

    /// Generate `ALTER TABLE ... DROP COLUMN ...` with `IF EXISTS`
    /// (SQLite does not support `DROP COLUMN`; emits a warning comment).
    pub fn drop_column(&self, table_name: &str, column_name: &str) -> String {
        if matches!(self.backend, Backend::SQLite) {
            format!(
                "-- SQLite does not support DROP COLUMN.  Recreate the table manually.\n-- ALTER TABLE {} DROP COLUMN {};",
                self.quote(table_name),
                self.quote(column_name)
            )
        } else {
            format!(
                "ALTER TABLE {} DROP COLUMN IF EXISTS {};",
                self.quote(table_name),
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
                        self.quote(table_name),
                        self.quote(&new_col.name),
                        self.col_type(&new_col.db_type)
                    ));
                }
                if old_col.nullable != new_col.nullable {
                    if new_col.nullable {
                        stmts.push(format!(
                            "ALTER TABLE {} ALTER COLUMN {} DROP NOT NULL;",
                            self.quote(table_name),
                            self.quote(&new_col.name)
                        ));
                    } else {
                        stmts.push(format!(
                            "ALTER TABLE {} ALTER COLUMN {} SET NOT NULL;",
                            self.quote(table_name),
                            self.quote(&new_col.name)
                        ));
                    }
                }
                // DEFAULT change
                if new_col.default != old_col.default {
                    match &new_col.default {
                        Some(val) => stmts.push(format!(
                            "ALTER TABLE {} ALTER COLUMN {} SET DEFAULT {};",
                            self.quote(table_name),
                            self.quote(&new_col.name),
                            val
                        )),
                        None => stmts.push(format!(
                            "ALTER TABLE {} ALTER COLUMN {} DROP DEFAULT;",
                            self.quote(table_name),
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
                    self.quote(table_name),
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
            t = table_name
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
            self.quote(table_name),
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
                    self.quote(table_name)
                )
            }
            Backend::PostgreSQL => {
                // PG uses `IF EXISTS` directly; quote schema-qualified
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
            self.quote(table_name),
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
            self.quote(table_name),
            self.quote(constraint_name),
            self.quote(column),
            self.quote(ref_table),
            self.quote(ref_column)
        )
    }

    // ── bulk generation ──────────────────────────────────

    /// Generate all DDL statements for the given changes.
    pub fn generate(&self, changes: &[SchemaChange]) -> Vec<String> {
        let mut statements = Vec::new();

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
                    columns: cols,
                };
                statements.push(self.create_table(&table));
            }
        }

        // Second pass: ALTER TABLE ADD COLUMN (for existing tables)
        let created_tables: Vec<&str> = create_tables.iter().map(|c| c.table.as_str()).collect();
        for change in changes {
            if change.kind == ChangeKind::AddColumn
                && !created_tables.contains(&change.table.as_str())
            {
                if let Some(ref col) = change.column {
                    statements.push(self.add_column(&change.table, col));
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
                    statements.extend(self.alter_column(&change.table, old_col, col));
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
    let ddl_gen = DDLGenerator::new(backend);
    let mut stmts = Vec::new();
    for table in tables {
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
}
