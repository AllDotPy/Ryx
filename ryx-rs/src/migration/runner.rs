use std::collections::HashSet;
use std::path::PathBuf;

use ryx_common::RyxResult;

use crate::migration::autodetect::{Autodetector, ModelEntry};
use crate::migration::ddl::DDLGenerator;
use crate::migration::files::{discover_migration_files, load_migration_file};
use crate::migration::operations::Operation;
use crate::migration::{
    diff_states, generate_ddl, get_text, introspect_schema, ColumnState, SchemaState, TableState,
    MIGRATIONS_TABLE,
};

/// File-based migration runner with per-alias tracking and live fallback.
///
/// ```ignore
/// use ryx_rs::migration::FileRunner;
///
/// FileRunner::new()
///     .model::<Post>()
///     .model::<Author>()
///     .migrations_dir("migrations/")
///     .run().await?;
/// ```
pub struct FileRunner {
    models: Vec<fn() -> TableState>,
    model_entries: Vec<ModelEntry>,
    db_alias: Option<String>,
    /// Database schema for PostgreSQL multi-schema support (empty = default).
    schema: String,
    migrations_dir: String,
    dry_run: bool,
    no_interactive: bool,
    /// When true, skip file discovery entirely and run a live auto-diff.
    live: bool,
}

impl FileRunner {
    pub fn new() -> Self {
        Self {
            models: vec![],
            model_entries: vec![],
            db_alias: None,
            schema: String::new(),
            migrations_dir: "migrations".to_string(),
            dry_run: false,
            no_interactive: false,
            live: false,
        }
    }

    pub fn db(mut self, alias: &str) -> Self {
        self.db_alias = Some(alias.to_string());
        self
    }

    /// Set the database schema for PostgreSQL multi-schema support.
    ///
    /// When set, all introspection, DDL, and operations will be scoped
    /// to this schema (e.g. ``CREATE TABLE "tenant1"."posts"``).
    /// Leave empty for default schema (no qualification).
    pub fn schema(mut self, schema: &str) -> Self {
        self.schema = schema.to_string();
        self
    }

    pub fn migrations_dir(mut self, dir: &str) -> Self {
        self.migrations_dir = dir.to_string();
        self
    }

    pub fn dry_run(mut self, dry: bool) -> Self {
        self.dry_run = dry;
        self
    }

    pub fn no_interactive(mut self, no: bool) -> Self {
        self.no_interactive = no;
        self
    }

    /// Skip file discovery — always run a live auto-diff against the DB.
    pub fn live(mut self, live: bool) -> Self {
        self.live = live;
        self
    }

    pub fn model<M: crate::model::Model>(mut self) -> Self {
        let info_fn = || -> TableState {
            let meta = M::field_meta();
            let columns: Vec<ColumnState> = meta.iter().map(|m| m.into()).collect();
            TableState {
                name: M::table_name().to_string(),
                schema: String::new(),
                columns,
            }
        };
        self.models.push(info_fn);
        let mut entry = ModelEntry::from_model::<M>();
        if !self.schema.is_empty() {
            entry = entry.with_schema(&self.schema);
        }
        self.model_entries.push(entry);
        self
    }

    fn build_target(&self) -> SchemaState {
        let mut tables: Vec<TableState> = self.models.iter().map(|f| f()).collect();
        if !self.schema.is_empty() {
            for table in &mut tables {
                table.schema = self.schema.clone();
            }
        }
        SchemaState { tables }
    }

    // ── Public API ──────────────────────────────────────

    pub async fn run(self) -> RyxResult<Vec<String>> {
        let alias = self.db_alias.clone().unwrap_or_else(|| "default".to_string());
        self.ensure_tracking_table(&alias).await?;

        if self.live {
            return self.run_live(&alias).await;
        }

        let dir = std::path::Path::new(&self.migrations_dir);
        let files = discover_migration_files(dir);

        if files.is_empty() {
            self.handle_no_files(&alias).await
        } else {
            self.apply_files(&alias, &files).await
        }
    }

    pub async fn plan(self) -> RyxResult<Vec<String>> {
        let alias = self.db_alias.clone().unwrap_or_else(|| "default".to_string());

        if self.live {
            return self.plan_live(&alias).await;
        }

        let dir = std::path::Path::new(&self.migrations_dir);
        let files = discover_migration_files(dir);

        if files.is_empty() {
            self.plan_live(&alias).await
        } else {
            self.plan_files(&alias, &files).await
        }
    }

    async fn run_live(&self, alias: &str) -> RyxResult<Vec<String>> {
        let target = self.build_target();
        let pool = ryx_backend::pool::get(Some(alias))?;
        let backend_type = ryx_backend::pool::get_backend(Some(alias))?;
        let current = introspect_schema(pool.as_ref(), backend_type, &self.schema).await?;
        let changes = diff_states(&current, &target);
        if changes.is_empty() {
            return Ok(vec![]);
        }
        let ddl = if self.schema.is_empty() {
            generate_ddl(&changes, backend_type)
        } else {
            DDLGenerator::new(backend_type)
                .in_schema(&self.schema)
                .generate(&changes)
        };
        let mut results = Vec::new();
        for stmt in &ddl {
            if self.dry_run {
                println!("{stmt}");
            } else {
                let _ = pool.fetch_raw(stmt.clone(), None).await?;
            }
            results.push(stmt.clone());
        }
        Ok(results)
    }

    async fn plan_live(&self, alias: &str) -> RyxResult<Vec<String>> {
        let pool = ryx_backend::pool::get(Some(alias))?;
        let backend_type = ryx_backend::pool::get_backend(Some(alias))?;
        let current = introspect_schema(pool.as_ref(), backend_type, &self.schema).await?;
        let target = self.build_target();
        let changes = diff_states(&current, &target);
        let ddl = if self.schema.is_empty() {
            generate_ddl(&changes, backend_type)
        } else {
            DDLGenerator::new(backend_type)
                .in_schema(&self.schema)
                .generate(&changes)
        };
        Ok(ddl)
    }

    // ── File pipeline ───────────────────────────────────

    async fn apply_files(
        &self,
        alias: &str,
        files: &[PathBuf],
    ) -> RyxResult<Vec<String>> {
        let pool = ryx_backend::pool::get(Some(alias))?;
        let backend_type = ryx_backend::pool::get_backend(Some(alias))?;
        let base_ddl = if self.schema.is_empty() {
            DDLGenerator::new(backend_type)
        } else {
            DDLGenerator::new(backend_type).in_schema(&self.schema)
        };
        let applied = self.get_applied_migrations(&pool, alias).await?;
        let mut results = Vec::new();

        for path in files {
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown");
            let key = format!("{alias}|{stem}");
            if applied.contains(&key) {
                continue;
            }

            let mf = match load_migration_file(path) {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("[ryx] Skipping {}: {e}", path.display());
                    continue;
                }
            };

            for op in &mf.operations {
                if !operation_is_relevant(op, alias) {
                    continue;
                }
                let mut ddl = base_ddl.clone();
                if !op.schema().is_empty() {
                    ddl.schema = op.schema().to_string();
                }
                for sql in operation_to_sql(&ddl, op) {
                    if self.dry_run {
                        println!("{sql}");
                    } else {
                        let _ = pool.fetch_raw(sql.clone(), None).await?;
                    }
                    results.push(sql);
                }
            }

            if !self.dry_run {
                self.record_migration(&pool, &key).await?;
            }
        }
        Ok(results)
    }

    async fn plan_files(
        &self,
        alias: &str,
        files: &[PathBuf],
    ) -> RyxResult<Vec<String>> {
        let pool = ryx_backend::pool::get(Some(alias))?;
        let backend_type = ryx_backend::pool::get_backend(Some(alias))?;
        let base_ddl = if self.schema.is_empty() {
            DDLGenerator::new(backend_type)
        } else {
            DDLGenerator::new(backend_type).in_schema(&self.schema)
        };
        let applied = self.get_applied_migrations(&pool, alias).await?;
        let mut results = Vec::new();

        for path in files {
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown");
            let key = format!("{alias}|{stem}");
            if applied.contains(&key) {
                continue;
            }
            if let Ok(mf) = load_migration_file(path) {
                for op in &mf.operations {
                    if operation_is_relevant(op, alias) {
                        let mut ddl = base_ddl.clone();
                        if !op.schema().is_empty() {
                            ddl.schema = op.schema().to_string();
                        }
                        results.extend(operation_to_sql(&ddl, op));
                    }
                }
            }
        }
        Ok(results)
    }

    // ── Tracking table ──────────────────────────────────

    async fn ensure_tracking_table(&self, alias: &str) -> RyxResult<()> {
        let pool = ryx_backend::pool::get(Some(alias))?;
        let sql = format!(
            "CREATE TABLE IF NOT EXISTS \"{t}\" (\
             id INTEGER PRIMARY KEY,\
             name VARCHAR(255) NOT NULL UNIQUE,\
             applied_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP\
             )",
            t = MIGRATIONS_TABLE
        );
        let _ = pool.fetch_raw(sql, None).await?;
        Ok(())
    }

    async fn get_applied_migrations(
        &self,
        pool: &std::sync::Arc<dyn ryx_backend::backends::RyxBackend>,
        alias: &str,
    ) -> RyxResult<HashSet<String>> {
        let sql = format!("SELECT name FROM \"{}\"", MIGRATIONS_TABLE);
        let rows = pool.fetch_raw(sql, None).await?;
        let mut set = HashSet::new();

        for row in &rows {
            if let Some(name) = get_text(row, "name") {
                if name.contains('|') {
                    set.insert(name);
                } else {
                    set.insert(format!("{alias}|{name}"));
                }
            }
        }
        Ok(set)
    }

    async fn record_migration(
        &self,
        pool: &std::sync::Arc<dyn ryx_backend::backends::RyxBackend>,
        key: &str,
    ) -> RyxResult<()> {
        let sql = format!(
            "INSERT OR REPLACE INTO \"{}\" (name) VALUES ('{key}')",
            MIGRATIONS_TABLE
        );
        let _ = pool.fetch_raw(sql, None).await?;
        Ok(())
    }

    // ── No-files fallback ───────────────────────────────

    async fn handle_no_files(&self, alias: &str) -> RyxResult<Vec<String>> {
        let target = self.build_target();
        let pool = ryx_backend::pool::get(Some(alias))?;
        let backend_type = ryx_backend::pool::get_backend(Some(alias))?;
        let current = introspect_schema(pool.as_ref(), backend_type, &self.schema).await?;
        let changes = diff_states(&current, &target);

        if changes.is_empty() {
            return Ok(vec![]);
        }
        if self.no_interactive {
            eprintln!(
                "[ryx] No migration files exist and --no-interactive is set.\n\
                 Run 'ryx makemigrations' first."
            );
            return Ok(vec![]);
        }
        self.interactive_menu(alias, &changes).await
    }

    async fn interactive_menu(
        &self,
        alias: &str,
        changes: &[crate::migration::SchemaChange],
    ) -> RyxResult<Vec<String>> {
        println!();
        println!(
            "[ryx] No migration files exist for database '{alias}'"
        );
        println!("  {} model(s) are not yet tracked.", self.models.len());
        println!();
        println!("  [L]ive DDL — apply changes directly (development only)");
        println!("  [A]uto-generate migration files, then migrate");
        println!("  [M]anual — run 'ryx makemigrations' first");
        println!("  [S]kip this database for now");
        println!();

        let mut choice = String::new();
        print!("[ryx] Choice (L/A/M/S) [S]: ");
        use std::io::Write;
        let _ = std::io::stdout().flush();
        let _ = std::io::stdin().read_line(&mut choice);
        let choice = choice.trim().to_uppercase();

        match choice.as_str() {
            "L" => {
                println!(
                    "[ryx] Applying {} live change(s) to {alias}",
                    changes.len()
                );
                let pool = ryx_backend::pool::get(Some(alias))?;
                let backend_type = ryx_backend::pool::get_backend(Some(alias))?;
                let ddl = generate_ddl(changes, backend_type);
                let mut results = Vec::new();
                for stmt in &ddl {
                    if self.dry_run {
                        println!("{stmt}");
                    } else {
                        let _ = pool.fetch_raw(stmt.clone(), None).await?;
                    }
                    results.push(stmt.clone());
                }
                Ok(results)
            }
            "A" => {
                let detector =
                    Autodetector::new(self.model_entries.clone(), &self.migrations_dir);
                let ops = detector.detect();
                if ops.is_empty() {
                    println!("[ryx] No changes detected.");
                    return Ok(vec![]);
                }
                let path = detector
                    .write_migration(&ops)
                    .map_err(|e| ryx_common::RyxError::Internal(e))?;
                println!("[ryx] Created migration: {}", path.display());

                let pool = ryx_backend::pool::get(Some(alias))?;
                let backend_type = ryx_backend::pool::get_backend(Some(alias))?;
                let ddl = DDLGenerator::new(backend_type);
                let mut results = Vec::new();
                let stem = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown");
                let key = format!("{alias}|{stem}");

                for op in &ops {
                    for sql in operation_to_sql(&ddl, op) {
                        if self.dry_run {
                            println!("{sql}");
                        } else {
                            let _ = pool.fetch_raw(sql.clone(), None).await?;
                        }
                        results.push(sql);
                    }
                }
                if !self.dry_run {
                    self.record_migration(&pool, &key).await?;
                }
                println!("[ryx]   applied");
                Ok(results)
            }
            "M" => {
                println!("[ryx] Run: ryx makemigrations --models <module>");
                println!("[ryx] Then run 'ryx migrate' again.");
                Ok(vec![])
            }
            _ => {
                println!("[ryx] Skipping database '{alias}'.");
                Ok(vec![])
            }
        }
    }
}

impl Default for FileRunner {
    fn default() -> Self {
        Self::new()
    }
}

// ── Helpers ────────────────────────────────────────────

/// Check whether an operation should run for a given database alias.
pub fn operation_is_relevant(op: &Operation, alias: &str) -> bool {
    match op.database() {
        Some(db) => db == alias,
        None => true,
    }
}

/// Convert an ``Operation`` to backend-aware DDL statements.
pub fn operation_to_sql(ddl: &DDLGenerator, op: &Operation) -> Vec<String> {
    match op {
        Operation::CreateSchema { schema_name, .. } => {
            let sql = ddl.create_schema(schema_name);
            if sql.is_empty() {
                vec![]
            } else {
                vec![sql]
            }
        }
        Operation::CreateTable {
            table_name, columns, ..
        } => {
            let cols: Vec<ColumnState> = columns
                .iter()
                .map(|c| ColumnState {
                    name: c.name.clone(),
                    db_type: c.db_type.clone(),
                    nullable: c.nullable,
                    primary_key: c.primary_key,
                    unique: c.unique,
                    default: c.default.clone(),
                })
                .collect();
            vec![ddl.create_table(&TableState {
                name: table_name.clone(),
                schema: ddl.schema.clone(),
                columns: cols,
            })]
        }
        Operation::AddField {
            table_name, column, ..
        } => {
            vec![ddl.add_column(
                table_name,
                &ColumnState {
                    name: column.name.clone(),
                    db_type: column.db_type.clone(),
                    nullable: column.nullable,
                    primary_key: column.primary_key,
                    unique: column.unique,
                    default: column.default.clone(),
                },
            )]
        }
        Operation::RemoveField {
            table_name,
            column_name,
            ..
        } => vec![ddl.drop_column(table_name, column_name)],
        Operation::AlterField {
            table_name,
            old_column,
            new_column,
            ..
        } => {
            ddl.alter_column(
                table_name,
                &ColumnState {
                    name: old_column.name.clone(),
                    db_type: old_column.db_type.clone(),
                    nullable: old_column.nullable,
                    primary_key: old_column.primary_key,
                    unique: old_column.unique,
                    default: old_column.default.clone(),
                },
                &ColumnState {
                    name: new_column.name.clone(),
                    db_type: new_column.db_type.clone(),
                    nullable: new_column.nullable,
                    primary_key: new_column.primary_key,
                    unique: new_column.unique,
                    default: new_column.default.clone(),
                },
            )
        }
        Operation::CreateIndex {
            table_name,
            index_name,
            fields,
            unique,
            ..
        } => vec![ddl.create_index(table_name, index_name, fields, *unique)],
        Operation::DeleteIndex {
            table_name,
            index_name,
            ..
        } => vec![ddl.drop_index(table_name, index_name)],
        Operation::RunSQL { sql, .. } => vec![sql.clone()],
    }
}
