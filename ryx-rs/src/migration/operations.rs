use serde::{Deserialize, Serialize};

/// A single migration operation, serializable to/from YAML migration files.
///
/// Each variant stores the target table name and an optional model/database
/// reference used by the runner for per-alias routing.
///
/// ```yaml
/// - type: CreateTable
///   table_name: authors
///   model_name: myapp::Author
///   database: blog
///   columns:
///     - { name: id, db_type: INTEGER, pk: true, unique: true }
///     - { name: name, db_type: VARCHAR(100), nullable: false }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum Operation {
    /// Create a new table with the given columns.
    #[serde(rename = "CreateTable")]
    CreateTable {
        table_name: String,
        columns: Vec<SerializedColumn>,
        #[serde(skip_serializing_if = "Option::is_none")]
        model_name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        database: Option<String>,
    },
    /// Add a column to an existing table.
    #[serde(rename = "AddField")]
    AddField {
        table_name: String,
        column: SerializedColumn,
        #[serde(skip_serializing_if = "Option::is_none")]
        model_name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        database: Option<String>,
    },
    /// Drop a column.  Destructive — not auto-generated.
    #[serde(rename = "RemoveField")]
    RemoveField {
        table_name: String,
        column_name: String,
    },
    /// Alter a column definition (type, nullability, etc.).
    #[serde(rename = "AlterField")]
    AlterField {
        table_name: String,
        old_column: SerializedColumn,
        new_column: SerializedColumn,
        #[serde(skip_serializing_if = "Option::is_none")]
        model_name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        database: Option<String>,
    },
    /// Create an index on one or more columns.
    #[serde(rename = "CreateIndex")]
    CreateIndex {
        table_name: String,
        index_name: String,
        fields: Vec<String>,
        unique: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        model_name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        database: Option<String>,
    },
    /// Drop an index.
    #[serde(rename = "DeleteIndex")]
    DeleteIndex {
        table_name: String,
        index_name: String,
    },
    /// Raw SQL to run forward.  ``reverse_sql`` is for rollback.
    /// Applies to all aliases (no table/model routing).
    #[serde(rename = "RunSQL")]
    RunSQL {
        sql: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        reverse_sql: Option<String>,
    },
}

/// A column definition suitable for YAML serialization.
///
/// Mirrors ``ColumnState`` but uses flat fields for readability.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SerializedColumn {
    pub name: String,
    pub db_type: String,
    #[serde(default)]
    pub nullable: bool,
    #[serde(default, rename = "pk")]
    pub primary_key: bool,
    #[serde(default)]
    pub unique: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
}

impl Operation {
    /// The target database alias for this operation, if set.
    ///
    /// Operations without a database are considered relevant for **all**
    /// aliases (legacy / raw-SQL fallback).
    pub fn database(&self) -> Option<&str> {
        match self {
            Operation::CreateTable { database, .. }
            | Operation::AddField { database, .. }
            | Operation::AlterField { database, .. }
            | Operation::CreateIndex { database, .. } => database.as_deref(),
            Operation::RemoveField { .. }
            | Operation::DeleteIndex { .. }
            | Operation::RunSQL { .. } => None,
        }
    }

    /// The model name this operation targets.
    pub fn model_name(&self) -> Option<&str> {
        match self {
            Operation::CreateTable { model_name, .. }
            | Operation::AddField { model_name, .. }
            | Operation::AlterField { model_name, .. }
            | Operation::CreateIndex { model_name, .. } => model_name.as_deref(),
            Operation::RemoveField { .. }
            | Operation::DeleteIndex { .. }
            | Operation::RunSQL { .. } => None,
        }
    }

    /// The table name this operation acts on.
    pub fn table_name(&self) -> Option<&str> {
        match self {
            Operation::CreateTable { table_name, .. }
            | Operation::AddField { table_name, .. }
            | Operation::RemoveField { table_name, .. }
            | Operation::AlterField { table_name, .. }
            | Operation::CreateIndex { table_name, .. }
            | Operation::DeleteIndex { table_name, .. } => Some(table_name.as_str()),
            Operation::RunSQL { .. } => None,
        }
    }
}
