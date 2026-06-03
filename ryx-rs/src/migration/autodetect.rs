use std::path::Path;

use crate::migration::files::{discover_migration_files, load_migration_file, write_migration_file};
use crate::migration::operations::{Operation, SerializedColumn};
use crate::migration::{diff_states, ColumnState, SchemaChange, SchemaState, TableState};

/// Describes one model known to the autodetector.
#[derive(Clone)]
pub struct ModelEntry {
    /// Human-readable name stored in YAML ``model_name`` (e.g. ``"myapp::Author"``).
    pub name: String,
    /// Table name (used to match operations to models).
    pub table_name: String,
    /// Database alias for routing — read from ``Model::database()``.
    pub database: String,
    /// Builds the target ``TableState`` for this model.
    pub make_state: fn() -> TableState,
}

impl ModelEntry {
    /// Create an entry from a ``Model`` implementation.
    ///
    /// ```ignore
    /// use ryx_rs::model::Model;
    /// use ryx_rs::migration::autodetect::ModelEntry;
    ///
    /// let entry = ModelEntry::from_model::<Author>();
    /// ```
    pub fn from_model<M: crate::model::Model>() -> Self {
        Self {
            name: M::table_name().to_string(),
            table_name: M::table_name().to_string(),
            database: M::database().to_string(),
            make_state: || -> TableState {
                let metas = M::field_meta();
                TableState {
                    name: M::table_name().to_string(),
                    columns: metas.iter().map(|m| m.into()).collect(),
                }
            },
        }
    }
}

/// Compares the state represented by migration files against a target
/// state built from model declarations, then generates new migration files.
#[derive(Clone)]
pub struct Autodetector {
    entries: Vec<ModelEntry>,
    migrations_dir: String,
}

impl Autodetector {
    pub fn new(entries: Vec<ModelEntry>, migrations_dir: &str) -> Self {
        Self {
            entries,
            migrations_dir: migrations_dir.to_string(),
        }
    }

    /// Build the **target** schema from the registered models.
    pub fn build_target(&self) -> SchemaState {
        let mut tables = Vec::new();
        for entry in &self.entries {
            tables.push((entry.make_state)());
        }
        SchemaState { tables }
    }

    /// Replay all existing migration files to reconstruct the "current"
    /// applied schema state.
    pub fn build_current(&self) -> SchemaState {
        let dir = Path::new(&self.migrations_dir);
        let files = discover_migration_files(dir);
        let mut state = SchemaState { tables: vec![] };

        for path in &files {
            if let Ok(mf) = load_migration_file(path) {
                state = Self::apply_operations(&state, &mf.operations);
            }
        }

        state
    }

    /// Apply a list of operations to a schema state and return the new state.
    ///
    /// This is used to "replay" migration files and compute what the database
    /// should look like after all applied migrations.
    fn apply_operations(state: &SchemaState, ops: &[Operation]) -> SchemaState {
        let mut tables = state.tables.clone();

        for op in ops {
            match op {
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
                    // Replace if already exists (idempotent replay)
                    tables.retain(|t| t.name != *table_name);
                    tables.push(TableState {
                        name: table_name.clone(),
                        columns: cols,
                    });
                }
                Operation::AddField {
                    table_name, column, ..
                } => {
                    if let Some(table) = tables.iter_mut().find(|t| t.name == *table_name) {
                        table.columns.push(ColumnState {
                            name: column.name.clone(),
                            db_type: column.db_type.clone(),
                            nullable: column.nullable,
                            primary_key: column.primary_key,
                            unique: column.unique,
                            default: column.default.clone(),
                        });
                    }
                }
                Operation::AlterField {
                    table_name,
                    old_column,
                    new_column,
                    ..
                } => {
                    if let Some(table) = tables.iter_mut().find(|t| t.name == *table_name) {
                        if let Some(col) = table
                            .columns
                            .iter_mut()
                            .find(|c| c.name == old_column.name)
                        {
                            col.db_type = new_column.db_type.clone();
                            col.nullable = new_column.nullable;
                            col.unique = new_column.unique;
                            col.default = new_column.default.clone();
                        }
                    }
                }
                Operation::RemoveField {
                    table_name,
                    column_name,
                } => {
                    if let Some(table) = tables.iter_mut().find(|t| t.name == *table_name) {
                        table.columns.retain(|c| c.name != *column_name);
                    }
                }
                // CreateIndex / DeleteIndex / RunSQL don't affect the schema state
                _ => {}
            }
        }

        SchemaState { tables }
    }

    /// Detect changes between the current (replayed) schema and the target
    /// schema, and return a list of YAML-ready ``Operation`` values.
    pub fn detect(&self) -> Vec<Operation> {
        let current = self.build_current();
        let target = self.build_target();
        let changes = diff_states(&current, &target);
        self.changes_to_operations(&changes)
    }

    /// Convert raw schema changes into migration ``Operation`` values,
    /// enriching each with model name and database alias where possible.
    ///
    /// Groups ``AddColumn`` changes for newly created tables into the
    /// corresponding ``CreateTable`` operation's column list.
    fn changes_to_operations(&self, changes: &[SchemaChange]) -> Vec<Operation> {
        let mut ops = Vec::new();

        // Collect tables being created so we know which AddColumns to merge
        let created_tables: Vec<&str> = changes
            .iter()
            .filter(|c| c.kind == crate::migration::ChangeKind::CreateTable)
            .map(|c| c.table.as_str())
            .collect();

        // Pass 1: CreateTable + its AddColumn children
        for ch in changes {
            if ch.kind == crate::migration::ChangeKind::CreateTable {
                let entry = self.find_entry(&ch.table);
                let cols: Vec<SerializedColumn> = changes
                    .iter()
                    .filter(|c| {
                        c.kind == crate::migration::ChangeKind::AddColumn
                            && c.table == ch.table
                            && c.column.is_some()
                    })
                    .filter_map(|c| {
                        c.column.as_ref().map(|col| SerializedColumn {
                            name: col.name.clone(),
                            db_type: col.db_type.clone(),
                            nullable: col.nullable,
                            primary_key: col.primary_key,
                            unique: col.unique,
                            default: col.default.clone(),
                        })
                    })
                    .collect();

                ops.push(Operation::CreateTable {
                    table_name: ch.table.clone(),
                    columns: cols,
                    model_name: entry.map(|e| e.name.clone()),
                    database: entry.map(|e| e.database.clone()),
                });
            }
        }

        // Pass 2: AddColumn for existing tables (not part of a CreateTable)
        for ch in changes {
            if ch.kind == crate::migration::ChangeKind::AddColumn
                && !created_tables.contains(&ch.table.as_str())
            {
                if let Some(col) = &ch.column {
                    let entry = self.find_entry(&ch.table);
                    ops.push(Operation::AddField {
                        table_name: ch.table.clone(),
                        column: SerializedColumn {
                            name: col.name.clone(),
                            db_type: col.db_type.clone(),
                            nullable: col.nullable,
                            primary_key: col.primary_key,
                            unique: col.unique,
                            default: col.default.clone(),
                        },
                        model_name: entry.map(|e| e.name.clone()),
                        database: entry.map(|e| e.database.clone()),
                    });
                }
            }
        }

        // Pass 3: AlterColumn
        for ch in changes {
            if ch.kind == crate::migration::ChangeKind::AlterColumn {
                if let (Some(col), Some(old_col)) = (&ch.column, &ch.old_column) {
                    let entry = self.find_entry(&ch.table);
                    ops.push(Operation::AlterField {
                        table_name: ch.table.clone(),
                        old_column: SerializedColumn {
                            name: old_col.name.clone(),
                            db_type: old_col.db_type.clone(),
                            nullable: old_col.nullable,
                            primary_key: old_col.primary_key,
                            unique: old_col.unique,
                            default: old_col.default.clone(),
                        },
                        new_column: SerializedColumn {
                            name: col.name.clone(),
                            db_type: col.db_type.clone(),
                            nullable: col.nullable,
                            primary_key: col.primary_key,
                            unique: col.unique,
                            default: col.default.clone(),
                        },
                        model_name: entry.map(|e| e.name.clone()),
                        database: entry.map(|e| e.database.clone()),
                    });
                }
            }
        }

        ops
    }

    /// Find the model entry for a given table name.
    fn find_entry(&self, table_name: &str) -> Option<&ModelEntry> {
        self.entries.iter().find(|e| e.table_name == table_name)
    }

    /// Write a migration file containing the given operations.
    ///
    /// Auto-numbers the filename and returns the created path.
    pub fn write_migration(&self, ops: &[Operation]) -> Result<std::path::PathBuf, String> {
        let dir = Path::new(&self.migrations_dir);
        write_migration_file(ops, dir)
    }

    /// Full pipeline: detect → write.  Returns the path of the new file,
    /// or ``None`` if no changes were detected.
    pub fn run(&self) -> Result<Option<std::path::PathBuf>, String> {
        let ops = self.detect();
        if ops.is_empty() {
            return Ok(None);
        }
        let path = self.write_migration(&ops)?;
        Ok(Some(path))
    }
}
