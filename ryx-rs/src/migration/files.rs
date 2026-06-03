use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::migration::operations::Operation;

/// A parsed migration file from disk.
///
/// ```yaml
/// dependencies: []
/// operations:
///   - type: CreateTable
///     table_name: authors
///     model_name: myapp::Author
///     database: blog
///     columns:
///       - { name: id, db_type: INTEGER, pk: true }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationFile {
    /// Migration names this one depends on.
    #[serde(default)]
    pub dependencies: Vec<String>,
    /// Operations to apply.
    pub operations: Vec<Operation>,
}

/// Load a migration file from a YAML path.
pub fn load_migration_file(path: &Path) -> Result<MigrationFile, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Cannot read {}: {}", path.display(), e))?;
    serde_yaml::from_str(&content)
        .map_err(|e| format!("Invalid YAML in {}: {}", path.display(), e))
}

/// Write operations to a new migration file.
///
/// Auto-numbers with the next available prefix (``0001_initial.yaml``,
/// ``0002_add_views.yaml``, etc.) and generates a slug from the first
/// operation's description.
pub fn write_migration_file(
    operations: &[Operation],
    migrations_dir: &Path,
) -> Result<PathBuf, String> {
    let next_number = next_migration_number(migrations_dir);
    let name = slug_from_operations(operations);
    let filename = format!("{:04}_{}.yaml", next_number, name);

    // Ensure directory exists
    std::fs::create_dir_all(migrations_dir)
        .map_err(|e| format!("Cannot create migrations dir: {}", e))?;

    let path = migrations_dir.join(&filename);
    let file = MigrationFile {
        dependencies: vec![],
        operations: operations.to_vec(),
    };
    let yaml = serde_yaml::to_string(&file)
        .map_err(|e| format!("Cannot serialize migration: {}", e))?;

    std::fs::write(&path, yaml)
        .map_err(|e| format!("Cannot write {}: {}", path.display(), e))?;

    Ok(path)
}

/// Recursively discover all migration files (``[0-9]*.yaml``) under a directory.
///
/// Files are sorted globally by stem so that ``0001_*`` always runs before
/// ``0002_*``, regardless of which subdirectory they live in.
pub fn discover_migration_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = Vec::new();

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && path.file_name().map(|n| n != "__pycache__").unwrap_or(true) {
                files.extend(discover_migration_files(&path));
            } else if path.is_file() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.ends_with(".yaml") || name.ends_with(".yml") {
                        if name.starts_with(|c: char| c.is_ascii_digit()) {
                            files.push(path);
                        }
                    }
                }
            }
        }
    }

    files.sort_by(|a, b| {
        let sa = a.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let sb = b.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        sa.cmp(sb)
    });

    files
}

// ── helpers ──────────────────────────────────────────────────

/// Return the next available migration number (1-based), scanning recursively.
fn next_migration_number(dir: &Path) -> u32 {
    let mut max_n = 0u32;

    fn scan(dir: &Path, max_n: &mut u32) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() && path.file_name().map(|n| n != "__pycache__").unwrap_or(true) {
                    scan(&path, max_n);
                } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if let Some(prefix) = name
                        .chars()
                        .take_while(|c| c.is_ascii_digit())
                        .collect::<String>()
                        .parse::<u32>()
                        .ok()
                    {
                        *max_n = (*max_n).max(prefix);
                    }
                }
            }
        }
    }

    scan(dir, &mut max_n);
    max_n + 1
}

/// Generate a human-readable slug from the first operation.
fn slug_from_operations(ops: &[Operation]) -> String {
    let desc = ops
        .first()
        .map(|op| match op {
            Operation::CreateTable { table_name, .. } => format!("create_{table_name}"),
            Operation::AddField {
                table_name, column, ..
            } => {
                format!("add_{}_{}", table_name, column.name)
            }
            Operation::AlterField {
                table_name, new_column, ..
            } => {
                format!("alter_{}_{}", table_name, new_column.name)
            }
            Operation::CreateIndex { index_name, .. } => format!("create_index_{index_name}"),
            Operation::RemoveField { table_name, column_name } => {
                format!("remove_{}_{}", table_name, column_name)
            }
            Operation::DeleteIndex { index_name, .. } => format!("delete_index_{index_name}"),
            Operation::RunSQL { .. } => "raw_sql".to_string(),
        })
        .unwrap_or_else(|| "auto".to_string());

    // Truncate to a reasonable length
    if desc.len() > 60 {
        let mut truncated = desc.chars().take(57).collect::<String>();
        truncated.push_str("...");
        truncated
    } else {
        desc
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migration::operations::SerializedColumn;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ryx_mig_{}_{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn test_write_and_load() {
        let dir = temp_dir("write_load");
        let ops = vec![Operation::CreateTable {
            table_name: "authors".into(),
            columns: vec![SerializedColumn {
                name: "id".into(),
                db_type: "INTEGER".into(),
                primary_key: true,
                unique: true,
                nullable: false,
                default: None,
            }],
            model_name: Some("test::Author".into()),
            database: Some("default".into()),
        }];

        let path = write_migration_file(&ops, &dir).unwrap();
        assert!(path.exists());
        assert!(path.file_name().unwrap().to_str().unwrap().starts_with("0001"));

        let loaded = load_migration_file(&path).unwrap();
        assert_eq!(loaded.operations.len(), 1);
        assert!(loaded.dependencies.is_empty());

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_discover_sorts_globally() {
        let dir = temp_dir("discover");
        let sub = dir.join("sub");
        std::fs::create_dir_all(&sub).unwrap();

        let ops = vec![Operation::CreateTable {
            table_name: "t".into(),
            columns: vec![],
            model_name: None,
            database: None,
        }];

        // File in subdirectory first (by numeric prefix)
        let _ = write_migration_file(&ops, &sub).unwrap();
        // Write another with the incremented number
        let _ = write_migration_file(&ops, &dir).unwrap();

        let files = discover_migration_files(&dir);
        assert_eq!(files.len(), 2);
        assert!(files[0].to_str().unwrap().contains("0001"));
        assert!(files[1].to_str().unwrap().contains("0002"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_slug_from_operations() {
        let ops = vec![Operation::CreateTable {
            table_name: "posts".into(),
            columns: vec![],
            model_name: None,
            database: None,
        }];
        assert_eq!(slug_from_operations(&ops), "create_posts");
    }
}
