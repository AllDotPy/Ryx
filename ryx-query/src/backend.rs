use serde::{Deserialize, Serialize};

/// Database backend type.
/// Used for backend-specific SQL generation (e.g., DATE() vs strftime()).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Backend {
    PostgreSQL,
    MySQL,
    SQLite,
}

impl Backend {
    pub fn as_str(&self) -> &'static str {
        match self {
            Backend::PostgreSQL => "postgres",
            Backend::MySQL => "mysql",
            Backend::SQLite => "sqlite",
        }
    }

    /// Whether this backend supports PostgreSQL-style schemas
    /// (namespaced tables within a single database).
    pub const fn supports_schemas(&self) -> bool {
        matches!(self, Backend::PostgreSQL)
    }
}

/// Detect the backend from a database URL.
pub fn detect_backend(url: &str) -> Backend {
    let url_lower = url.to_lowercase();
    if url_lower.contains("postgres") {
        Backend::PostgreSQL
    } else if url_lower.contains("mysql") {
        Backend::MySQL
    } else if url_lower.contains("sqlite") {
        Backend::SQLite
    } else {
        Backend::PostgreSQL // default
    }
}
