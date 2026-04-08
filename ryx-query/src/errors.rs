use thiserror::Error;

#[derive(Debug, Error)]
pub enum QueryError {
    #[error("Unknown lookup: '{lookup}' on field '{field}'")]
    UnknownLookup { field: String, lookup: String },

    #[error("Unknown field '{field}' on model '{model}'")]
    UnknownField { field: String, model: String },

    #[error("Type mismatch for field '{field}': expected {expected}, got {got}")]
    TypeMismatch {
        field: String,
        expected: String,
        got: String,
    },

    #[error("Internal query error: {0}")]
    Internal(String),
}

pub type QueryResult<T> = Result<T, QueryError>;
