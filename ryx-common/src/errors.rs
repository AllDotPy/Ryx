use ryx_query::QueryError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RyxError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Database error: {1} (sql: {0})")]
    DatabaseWithSql(String, sqlx::Error),

    #[error("Query error: {0}")]
    Query(#[from] QueryError),

    #[error("No matching object found for the given query")]
    DoesNotExist,

    #[error("Query returned multiple objects; expected exactly one")]
    MultipleObjectsReturned,

    #[error("Connection pool is not initialized. Call setup() first.")]
    PoolNotInitialized,

    #[error("Connection pool already initialized")]
    PoolAlreadyInitialized,

    #[error("Internal Ryx error: {0}")]
    Internal(String),
}

pub type RyxResult<T> = Result<T, RyxError>;
