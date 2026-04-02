//
// ──────────────────────────────────────────────────────────────────────────────
// Ryx — Unified Error Type
// ──────────────────────────────────────────────────────────────────────────────
//
// Design decision: we define a single RyxError enum that covers every failure
// mode across the entire crate (database errors, type mapping errors, pool
// errors, etc.). This enum implements:
//
//   1. `thiserror::Error`  → gives us Display + Error + From impls for free
//   2. `From<RyxError> for PyErr`  → converts every Rust error into the
//      appropriate Python exception transparently (PyO3 calls this when a
//      #[pyfunction] returns Err(RyxError))
//
// We map Rust errors to Python exception types that users already know:
//   - DoesNotExist      → raises `Ryx.exceptions.DoesNotExist` (like Django)
//   - MultipleObjects   → raises `Ryx.exceptions.MultipleObjectsReturned`
//   - DatabaseError     → raises `Ryx.exceptions.DatabaseError`
//   - ...
//
// This keeps the Python surface clean: users never see "PyRuntimeError: sqlx::…"
// ──────────────────────────────────────────────────────────────────────────────

use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use thiserror::Error;

/// The master error type for the entire Ryx ORM.
///
/// Every function in this crate that can fail returns `Result<T, RyxError>`.
/// PyO3 automatically converts this into a Python exception via the `From` impl
/// below whenever a `#[pyfunction]` or `#[pymethods]` method returns `Err(...)`.
#[derive(Debug, Error)]
pub enum RyxError {
    // Database-level errors 

    /// Wraps every error produced by sqlx (connection failures, query errors,
    /// constraint violations, etc.). We keep the original sqlx error so that
    /// tracing/logging can capture the full details.
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    /// Raised when `.get()` or `.first()` finds no matching row.
    /// Mirrors Django's `Model.DoesNotExist`.
    #[error("No matching object found for the given query")]
    DoesNotExist,

    /// Raised when `.get()` matches more than one row.
    /// Mirrors Django's `Model.MultipleObjectsReturned`.
    #[error("Query returned multiple objects; expected exactly one")]
    MultipleObjectsReturned,

    // Connection pool errors 

    /// Raised when user code calls any ORM operation before `Ryx.setup()`
    /// has been called to initialize the connection pool.
    #[error("Connection pool is not initialized. Call Ryx.setup() first.")]
    PoolNotInitialized,

    /// Raised when the connection pool was already initialized and the user
    /// calls `Ryx.setup()` a second time with a different URL.
    #[error("Connection pool already initialized")]
    PoolAlreadyInitialized,

    // Query building errors 

    /// Raised when the Python side passes an unrecognized lookup suffix.
    /// Example: `filter(age__foobar=42)` where "foobar" is not a registered
    /// lookup. We include the lookup name so the error is actionable.
    #[error("Unknown lookup: '{lookup}' on field '{field}'")]
    UnknownLookup { field: String, lookup: String },

    /// Raised when a field name referenced in a filter/order_by doesn't exist
    /// on the model's declared schema.
    #[error("Unknown field '{field}' on model '{model}'")]
    UnknownField { field: String, model: String },

    /// Raised when a Python value cannot be converted to the expected SQL type.
    /// Example: passing a string where an integer is expected.
    #[error("Type mismatch for field '{field}': expected {expected}, got {got}")]
    TypeMismatch {
        field: String,
        expected: String,
        got: String,
    },

    // Runtime / internal errors 

    /// Catch-all for internal errors that shouldn't reach users but are
    /// wrapped here so we don't use `.unwrap()` anywhere in the codebase.
    /// If this appears in production, it's always a bug — please file an issue.
    #[error("Internal Ryx error: {0}")]
    Internal(String),
}

// ──────────────────────────────────────────────────────────────────────────────
// PyO3 conversion: RyxError → Python exception
//
// PyO3 requires `From<RyxError> for PyErr` so that functions marked
// `-> PyResult<T>` can use `?` to propagate RyxError automatically.
//
// We deliberately keep Python exception types simple and familiar:
//   - Lookup / field errors  → ValueError (user code problem)
//   - DoesNotExist           → RuntimeError (matches Django behaviour)
//   - Everything else        → RuntimeError with full message
//
// TODO: In a future version we should define custom Python exception classes
// (via `pyo3::create_exception!`) so users can do `except Ryx.DoesNotExist`.
// For now we keep it simple to avoid complexity in the foundation layer.
// ──────────────────────────────────────────────────────────────────────────────
impl From<RyxError> for PyErr {
    fn from(err: RyxError) -> PyErr {
        match &err {
            // User errors (bad field names, bad lookups, bad types) →
            // ValueError so Python linters/type checkers can catch them
            RyxError::UnknownLookup { .. }
            | RyxError::UnknownField { .. }
            | RyxError::TypeMismatch { .. } => PyValueError::new_err(err.to_string()),

            // Everything else → RuntimeError with full context message
            _ => PyRuntimeError::new_err(err.to_string()),
        }
    }
}

/// Convenience type alias used throughout the crate.
/// Every Ryx function returns `RyxResult<T>` instead of `Result<T, RyxError>`.
pub type RyxResult<T> = Result<T, RyxError>;