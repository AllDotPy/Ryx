// ###
// Ryx — Query Executor
// ###

// The executor is the bridge between our compiled SQL string and the live
// database. It:
//   1. Retrieves the global connection pool
//   2. Builds a sqlx query by binding `CompiledQuery.values` in order
//   3. Executes the query via sqlx's async API
//   4. Decodes each result row into a `HashMap<String, serde_json::Value>`
//      which is then converted to a Python dict on the PyO3 boundary

// # Why HashMap<String, serde_json::Value> as the row type?

// We need to pass row data back to Python as a dict. Using `serde_json::Value`
// as the intermediate representation lets us:
//   - Handle any SQL type (TEXT, INTEGER, FLOAT, BOOLEAN, NULL, JSON)
//   - Serialize/deserialize via serde without manual match arms per-column
//   - Convert to PyDict cleanly in the PyO3 layer

// The alternative — using PyDict directly in the Rust executor — would require
// holding the GIL for the entire query execution, which would block Python's
// event loop. By decoding to a Rust data structure first and converting only
// at the end, we minimize GIL hold time.

// # Value binding strategy

// sqlx's `AnyPool` requires values to be bound with `.bind()` and each value
// must implement `sqlx::Encode<sqlx::Any>`. Our `SqlValue` enum covers the
// full set of types we support, so we match on it and call `.bind()` for each
// variant.

// # Transaction support

// The executor works against any `sqlx::Executor` — either the pool directly
// or a `Transaction`. This lets us share execution logic between the regular
// path and the transactional path without code duplication.
// ###

use std::collections::HashMap;

use serde_json::Value as JsonValue;
use sqlx::{Column, Row, any::AnyRow};
use tracing::{debug, instrument};

use crate::errors::{RyxError, RyxResult};
use crate::pool;
use crate::query::{ast::SqlValue, compiler::CompiledQuery};
use crate::transaction;

// ###
// Result types
// ###

/// A single decoded database row: column name → JSON-compatible value.
///
/// Using `serde_json::Value` lets us represent NULL, integers, floats, strings,
/// and booleans without a custom enum. JSON values convert cleanly to Python
/// objects in the PyO3 layer.
pub type DecodedRow = HashMap<String, JsonValue>;

/// Result of a non-SELECT query (INSERT/UPDATE/DELETE).
#[derive(Debug)]
pub struct MutationResult {
    /// Number of rows affected.
    pub rows_affected: u64,
    /// The last inserted row's ID, if the query was an INSERT with
    /// `returning_id = true` and the database supports it.
    pub last_insert_id: Option<i64>,
}

// ###
// Public API
// ###

/// Execute a SELECT query and return all matching rows.
///
/// # Errors
/// - [`RyxError::PoolNotInitialized`] if `Ryx.setup()` hasn't been called
/// - [`RyxError::Database`] for SQL errors, connection failures, etc.
#[instrument(skip(query), fields(sql = %query.sql))]
pub async fn fetch_all(query: CompiledQuery) -> RyxResult<Vec<DecodedRow>> {
    if let Some(tx) = transaction::get_current_transaction() {
        let tx_guard = tx.lock().await;
        if let Some(active_tx) = tx_guard.as_ref() {
            return active_tx.fetch_query(query).await;
        }
        return Err(RyxError::Internal("Transaction is no longer active".into()));
    }

    let pool = pool::get()?;

    debug!(sql = %query.sql, "Executing SELECT");

    // Build the sqlx query and bind all values.
    // We use `sqlx::query()` (the dynamic version) because our SQL is
    // constructed at runtime — we can't use the compile-time `query!` macro.
    let mut q = sqlx::query(&query.sql);
    q = bind_values(q, &query.values);

    // Fetch all rows and decode each one into a DecodedRow.
    let rows = q.fetch_all(pool).await.map_err(RyxError::Database)?;

    let decoded = rows.iter().map(decode_row).collect();
    Ok(decoded)
}

/// Execute a SELECT COUNT(*) query and return the count.
///
/// # Errors
/// Same as [`fetch_all`].
#[instrument(skip(query), fields(sql = %query.sql))]
pub async fn fetch_count(query: CompiledQuery) -> RyxResult<i64> {
    if let Some(tx) = transaction::get_current_transaction() {
        let tx_guard = tx.lock().await;
        if let Some(active_tx) = tx_guard.as_ref() {
            let rows = active_tx.fetch_query(query).await?;
            if rows.is_empty() {
                return Ok(0);
            }
            // COUNT() returns a single column whose name may vary by backend.
            if let Some(value) = rows[0].values().next() {
                if let Some(i) = value.as_i64() {
                    return Ok(i);
                }
                if let Some(f) = value.as_f64() {
                    return Ok(f as i64);
                }
            }
            return Err(RyxError::Internal(
                "COUNT() returned unexpected value".into(),
            ));
        }
        return Err(RyxError::Internal("Transaction is no longer active".into()));
    }

    let pool = pool::get()?;

    debug!(sql = %query.sql, "Executing COUNT");

    let mut q = sqlx::query(&query.sql);
    q = bind_values(q, &query.values);

    let row = q.fetch_one(pool).await.map_err(RyxError::Database)?;

    // COUNT(*) always returns a single column. We try to get it as i64
    // first (Postgres/SQLite), then fall back to i32 (some MySQL versions).
    let count: i64 = row.try_get(0).unwrap_or_else(|_| {
        let n: i32 = row.try_get(0).unwrap_or(0);
        n as i64
    });

    Ok(count)
}

/// Execute a SELECT and return at most one row.
///
/// # Errors
/// - [`RyxError::DoesNotExist`] if no rows are found
/// - [`RyxError::MultipleObjectsReturned`] if more than one row is found
///
/// This mirrors Django's `.get()` semantics exactly.
#[instrument(skip(query), fields(sql = %query.sql))]
pub async fn fetch_one(query: CompiledQuery) -> RyxResult<DecodedRow> {
    // We intentionally fetch up to 2 rows to detect MultipleObjectsReturned
    // without fetching the entire result set. This is more efficient than
    // `fetch_all` when the user calls `.get()` on a large table.
    if let Some(tx) = transaction::get_current_transaction() {
        let tx_guard = tx.lock().await;
        if let Some(active_tx) = tx_guard.as_ref() {
            let rows = active_tx.fetch_query(query).await?;
            match rows.len() {
                0 => Err(RyxError::DoesNotExist),
                1 => Ok(rows.into_iter().next().unwrap()),
                _ => Err(RyxError::MultipleObjectsReturned),
            }
        } else {
            Err(RyxError::Internal("Transaction is no longer active".into()))
        }
    } else {
        let pool = pool::get()?;

        let mut q = sqlx::query(&query.sql);
        q = bind_values(q, &query.values);

        // Limit to 2 at the executor level (the QueryNode may already have
        // LIMIT 1 set by `.first()`, but for `.get()` it doesn't).
        // We check the count in Rust rather than adding SQL complexity.
        let rows = q.fetch_all(pool).await.map_err(RyxError::Database)?;

        match rows.len() {
            0 => Err(RyxError::DoesNotExist),
            1 => Ok(decode_row(&rows[0])),
            _ => Err(RyxError::MultipleObjectsReturned),
        }
    }
}

/// Execute an INSERT, UPDATE, or DELETE query.
///
/// For INSERT queries with `RETURNING` clause, this fetches the returned
/// value and populates `last_insert_id`.
///
/// # Errors
/// - [`RyxError::PoolNotInitialized`]
/// - [`RyxError::Database`]
#[instrument(skip(query), fields(sql = %query.sql))]
pub async fn execute(query: CompiledQuery) -> RyxResult<MutationResult> {
    if let Some(tx) = transaction::get_current_transaction() {
        let tx_guard = tx.lock().await;
        if let Some(active_tx) = tx_guard.as_ref() {
            // Check if this is a RETURNING query
            if query.sql.to_uppercase().contains("RETURNING") {
                let rows = active_tx.fetch_query(query).await?;
                let last_insert_id = rows
                    .first()
                    .and_then(|row| row.values().next().and_then(|v| v.as_i64()));
                return Ok(MutationResult {
                    rows_affected: 1,
                    last_insert_id,
                });
            }
            let rows_affected = active_tx.execute_query(query).await?;
            return Ok(MutationResult {
                rows_affected,
                last_insert_id: None,
            });
        }
        return Err(RyxError::Internal("Transaction is no longer active".into()));
    }

    let pool = pool::get()?;

    debug!(sql = %query.sql, "Executing mutation");

    // Check if this is a RETURNING query (e.g. INSERT ... RETURNING id)
    if query.sql.to_uppercase().contains("RETURNING") {
        let mut q = sqlx::query(&query.sql);
        q = bind_values(q, &query.values);

        let rows = q.fetch_all(pool).await.map_err(RyxError::Database)?;

        let last_insert_id = rows.first().and_then(|row| row.try_get::<i64, _>(0).ok());

        return Ok(MutationResult {
            rows_affected: rows.len() as u64,
            last_insert_id,
        });
    }

    let mut q = sqlx::query(&query.sql);
    q = bind_values(q, &query.values);

    let result = q.execute(pool).await.map_err(RyxError::Database)?;

    Ok(MutationResult {
        rows_affected: result.rows_affected(),
        last_insert_id: None,
    })
}

// ###
// Internal helpers
// ###

/// Bind all `SqlValue`s to a sqlx query in order.
///
/// sqlx's `.bind()` takes ownership and returns a new query, so we chain
/// calls with a mutable variable rather than a functional fold to keep the
/// code readable.
fn bind_values<'q>(
    mut q: sqlx::query::Query<'q, sqlx::Any, sqlx::any::AnyArguments<'q>>,
    values: &'q [SqlValue],
) -> sqlx::query::Query<'q, sqlx::Any, sqlx::any::AnyArguments<'q>> {
    for value in values {
        q = match value {
            SqlValue::Null => q.bind(None::<String>),
            SqlValue::Bool(b) => q.bind(*b),
            SqlValue::Int(i) => q.bind(*i),
            SqlValue::Float(f) => q.bind(*f),
            SqlValue::Text(s) => q.bind(s.as_str()),
            // Lists should have been expanded by the compiler into individual
            // placeholders. If we encounter a List here it's a compiler bug.
            SqlValue::List(_) => {
                // This is a defensive no-op — the compiler should have expanded
                // lists already. We log a warning and skip.
                tracing::warn!("Unexpected List value reached executor — this is a compiler bug");
                q
            }
        };
    }
    q
}

/// Decode a single `AnyRow` into a `DecodedRow` (HashMap<String, JsonValue>).
///
/// We iterate over the columns and use sqlx's `try_get` to extract each value.
/// The `Any` database driver supports a limited set of types natively:
///   - i64 (maps to Bool and Int as well)
///   - f64
///   - String
///   - Vec<u8> (bytes)
///   - bool
///
/// We try each type in order and fall back to String if nothing else works.
fn decode_row(row: &AnyRow) -> DecodedRow {
    let mut map = HashMap::new();

    for column in row.columns() {
        let name = column.name().to_string();

        // Try to extract values in type priority order.
        // On SQLite, booleans are stored as INTEGER (0/1), so we try i64 first
        // and then check if the value looks like a bool.
        // On Postgres/MySQL, bool columns decode as bool natively.
        //
        // null: sqlx signals NULL by returning an Err on every typed get.
        // We detect this by trying Option<String> last.

        let value: JsonValue = if let Ok(i) = row.try_get::<i64, _>(column.ordinal()) {
            // Check if this column name suggests a boolean
            let col_lower = name.to_lowercase();
            let looks_bool = col_lower.starts_with("is_")
                || col_lower.starts_with("has_")
                || col_lower.starts_with("can_")
                || col_lower.ends_with("_flag");
            if looks_bool && (i == 0 || i == 1) {
                JsonValue::Bool(i != 0)
            } else {
                JsonValue::Number(i.into())
            }
        } else if let Ok(b) = row.try_get::<bool, _>(column.ordinal()) {
            JsonValue::Bool(b)
        } else if let Ok(f) = row.try_get::<f64, _>(column.ordinal()) {
            serde_json::Number::from_f64(f)
                .map(JsonValue::Number)
                .unwrap_or(JsonValue::Null)
        } else if let Ok(s) = row.try_get::<String, _>(column.ordinal()) {
            JsonValue::String(s)
        } else {
            // Either NULL or a type we don't handle — represent as null.
            JsonValue::Null
        };

        map.insert(name, value);
    }

    map
}
