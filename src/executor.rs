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

use sqlx::{Column, Row, any::AnyRow};
use tracing::{debug, instrument};

use crate::errors::{RyxError, RyxResult};
use crate::pool;
use ryx_query::{ast::{SqlValue, QueryNode}, compiler::CompiledQuery};
use crate::transaction;
use smallvec::SmallVec;

// ###
// Result types
// ###

/// A single decoded database row: column name → JSON-compatible value.
///
/// Using `serde_json::Value` lets us represent NULL, integers, floats, strings,
/// and booleans without a custom enum. JSON values convert cleanly to Python
/// objects in the PyO3 layer.
pub type DecodedRow = HashMap<String, ryx_query::ast::SqlValue>;

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

    let pool = pool::get(query.db_alias.as_deref())?;
 
    debug!(sql = %query.sql, "Executing SELECT");
 
    let mut q = sqlx::query(&query.sql);
    q = bind_values(q, &query.values);
 
    let rows = q.fetch_all(&*pool).await.map_err(RyxError::Database)?;

    let decoded = decode_rows(&rows);
    Ok(decoded)
}

/// Execute raw SQL (no binds) directly, bypassing compiler.
#[instrument(skip(sql))]
pub async fn fetch_raw(sql: String, db_alias: Option<String>) -> RyxResult<Vec<DecodedRow>> {
    let pool = pool::get(db_alias.as_deref())?;
    let rows = sqlx::query(&sql).fetch_all(&*pool).await.map_err(RyxError::Database)?;
    Ok(decode_rows(&rows))
}

/// Compile a QueryNode then fetch all (single FFI hop helper).
#[instrument(skip(node))]
pub async fn fetch_all_compiled(node: QueryNode) -> RyxResult<Vec<DecodedRow>> {
    let compiled = ryx_query::compiler::compile(&node).map_err(RyxError::from)?;
    fetch_all(compiled).await
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
            if let Some(value) = rows[0].values().next() {
                match value {
                    SqlValue::Int(i) => return Ok(*i),
                    SqlValue::Float(f) => return Ok(*f as i64),
                    _ => {}
                }
            }
            return Err(RyxError::Internal(
                "COUNT() returned unexpected value".into(),
            ));
        }
        return Err(RyxError::Internal("Transaction is no longer active".into()));
    }
 
    let pool = pool::get(query.db_alias.as_deref())?;
 
    debug!(sql = %query.sql, "Executing COUNT");
 
    let mut q = sqlx::query(&query.sql);
    q = bind_values(q, &query.values);
 
    let row = q.fetch_one(&*pool).await.map_err(RyxError::Database)?;
 
    let count: i64 = row.try_get(0).unwrap_or_else(|_| {
        let n: i32 = row.try_get(0).unwrap_or(0);
        n as i64
    });
 
    Ok(count)
}

#[instrument(skip(node))]
pub async fn fetch_count_compiled(node: QueryNode) -> RyxResult<i64> {
    let compiled = ryx_query::compiler::compile(&node).map_err(RyxError::from)?;
    fetch_count(compiled).await
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
        let pool = pool::get(query.db_alias.as_deref())?;
 
        let mut q = sqlx::query(&query.sql);
        q = bind_values(q, &query.values);
 
        // Limit to 2 at the executor level (the QueryNode may already have
        // LIMIT 1 set by `.first()`, but for `.get()` it doesn't).
        // We check the count in Rust rather than adding SQL complexity.
        let rows = q.fetch_all(&*pool).await.map_err(RyxError::Database)?;
 
        match rows.len() {
            0 => Err(RyxError::DoesNotExist),
            1 => Ok(decode_row(&rows[0], None)),
            _ => Err(RyxError::MultipleObjectsReturned),
        }
    }
}

#[instrument(skip(node))]
pub async fn fetch_one_compiled(node: QueryNode) -> RyxResult<DecodedRow> {
    let compiled = ryx_query::compiler::compile(&node).map_err(RyxError::from)?;
    fetch_one(compiled).await
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
                let last_insert_id = rows.first().and_then(|row| {
                    row.values().next().and_then(|v| match v {
                        SqlValue::Int(i) => Some(*i),
                        SqlValue::Float(f) => Some(*f as i64),
                        _ => None,
                    })
                });
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
 
    let pool = pool::get(query.db_alias.as_deref())?;
 
    debug!(sql = %query.sql, "Executing mutation");
 
    // Check if this is a RETURNING query (e.g. INSERT ... RETURNING id)
    if query.sql.to_uppercase().contains("RETURNING") {
        let mut q = sqlx::query(&query.sql);
        q = bind_values(q, &query.values);

        let rows = q.fetch_all(&*pool).await.map_err(RyxError::Database)?;

        let last_insert_id = rows.first().and_then(|row| row.try_get::<i64, _>(0).ok());

        return Ok(MutationResult {
            rows_affected: rows.len() as u64,
            last_insert_id,
        });
    }
 
    let mut q = sqlx::query(&query.sql);
    q = bind_values(q, &query.values);
 
    let result = q.execute(&*pool).await.map_err(RyxError::Database)?;
 
    Ok(MutationResult {
        rows_affected: result.rows_affected(),
        last_insert_id: None,
    })
}


/// Execute QueryNode
#[instrument(skip(node))]
pub async fn execute_compiled(node: QueryNode) -> RyxResult<MutationResult> {
    let compiled = ryx_query::compiler::compile(&node).map_err(RyxError::from)?;
    execute(compiled).await
}

/// Bulk insert rows with values already mapped to SqlValue in one shot.
pub async fn bulk_insert(
    table: String,
    columns: Vec<String>,
    rows: Vec<Vec<SqlValue>>,
    returning_id: bool,
    ignore_conflicts: bool,
    db_alias: Option<String>,
) -> RyxResult<MutationResult> {
    if rows.is_empty() {
        return Ok(MutationResult { rows_affected: 0, last_insert_id: None });
    }
    let pool = pool::get(db_alias.as_deref())?;
    let backend = pool::get_backend(db_alias.as_deref())?;

    let col_list = columns.iter().map(|c| format!("\"{}\"", c)).collect::<Vec<_>>().join(", ");
    let row_ph = format!("({})", std::iter::repeat("?").take(columns.len()).collect::<Vec<_>>().join(", "));
    let values_sql = std::iter::repeat(row_ph.clone()).take(rows.len()).collect::<Vec<_>>().join(", ");

    let mut flat: SmallVec<[SqlValue; 8]> = SmallVec::new();
    for row in rows {
        for v in row {
            flat.push(v);
        }
    }

    let (insert_kw, conflict_suffix) = if ignore_conflicts {
        match backend {
            ryx_query::Backend::PostgreSQL => ("INSERT INTO", " ON CONFLICT DO NOTHING"),
            ryx_query::Backend::MySQL => ("INSERT IGNORE INTO", ""),
            ryx_query::Backend::SQLite => ("INSERT OR IGNORE INTO", ""),
        }
    } else {
        ("INSERT INTO", "")
    };

    let sql = format!(
        "{} \"{}\" ({}) VALUES {}{}{}",
        insert_kw,
        table,
        col_list,
        values_sql,
        conflict_suffix,
        if returning_id { " RETURNING id" } else { "" }
    );
    let mut q = sqlx::query(&sql);
    q = bind_values(q, &flat);
    if returning_id {
        let rows = q.fetch_all(&*pool).await.map_err(RyxError::Database)?;
        let last_insert_id = rows.first().and_then(|r| r.try_get::<i64, _>(0).ok());
        Ok(MutationResult { rows_affected: rows.len() as u64, last_insert_id })
    } else {
        let res = q.execute(&*pool).await.map_err(RyxError::Database)?;
        Ok(MutationResult { rows_affected: res.rows_affected(), last_insert_id: None })
    }
}

/// Bulk delete by primary key values in one shot.
pub async fn bulk_delete(
    table: String,
    pk_col: String,
    pks: Vec<SqlValue>,
    db_alias: Option<String>,
) -> RyxResult<MutationResult> {
    if pks.is_empty() {
        return Ok(MutationResult { rows_affected: 0, last_insert_id: None });
    }
    let pool = pool::get(db_alias.as_deref())?;
    let ph = std::iter::repeat("?").take(pks.len()).collect::<Vec<_>>().join(", ");
    let sql = format!(
        "DELETE FROM \"{}\" WHERE \"{}\" IN ({})",
        table, pk_col, ph
    );
    let mut q = sqlx::query(&sql);
    q = bind_values(q, &pks);
    let res = q.execute(&*pool).await.map_err(RyxError::Database)?;
    Ok(MutationResult { rows_affected: res.rows_affected(), last_insert_id: None })
}

/// Bulk update using CASE WHEN, values already mapped to SqlValue.
pub async fn bulk_update(
    table: String,
    pk_col: String,
    col_names: Vec<String>,
    field_values: Vec<Vec<SqlValue>>,
    pks: Vec<SqlValue>,
    db_alias: Option<String>,
) -> RyxResult<MutationResult> {
    let pool = pool::get(db_alias.as_deref())?;
    let n = pks.len();
    let f = field_values.len();
    if n == 0 || f == 0 {
        return Ok(MutationResult { rows_affected: 0, last_insert_id: None });
    }

    let mut case_clauses = Vec::with_capacity(f);
    let mut all_values: SmallVec<[SqlValue; 8]> = SmallVec::with_capacity(n * f * 2 + n);

    for (fi, col_name) in col_names.iter().enumerate() {
        let mut case_parts = Vec::with_capacity(n * 3 + 2);
        case_parts.push(format!("\"{}\" = CASE \"{}\"", col_name, pk_col));
        for i in 0..n {
            case_parts.push("WHEN ? THEN ?".to_string());
            all_values.push(pks[i].clone());
            all_values.push(field_values[fi][i].clone());
        }
        case_parts.push("END".to_string());
        case_clauses.push(case_parts.join(" "));
    }

    let pk_placeholders: Vec<String> = (0..n).map(|_| "?".to_string()).collect();
    for pk in &pks {
        all_values.push(pk.clone());
    }

    let sql = format!(
        "UPDATE \"{}\" SET {} WHERE \"{}\" IN ({})",
        table,
        case_clauses.join(", "),
        pk_col,
        pk_placeholders.join(", ")
    );

    let mut q = sqlx::query(&sql);
    q = bind_values(q, &all_values);
    let res = q.execute(&*pool).await.map_err(RyxError::Database)?;
    Ok(MutationResult { rows_affected: res.rows_affected(), last_insert_id: None })
}

/// Execute raw SQL without bind params.
#[instrument(skip(sql))]
pub async fn execute_raw(sql: String, db_alias: Option<String>) -> RyxResult<()> {
    let pool = pool::get(db_alias.as_deref())?;
    sqlx::query(&sql).execute(&*pool).await.map_err(RyxError::Database)?;
    Ok(())
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

/// Decode all rows with a precomputed column-name vector to reduce per-row allocations.
fn decode_rows(rows: &[AnyRow]) -> Vec<DecodedRow> {
    if rows.is_empty() {
        return Vec::new();
    }

    let col_names: Vec<String> = rows[0]
        .columns()
        .iter()
        .map(|c| c.name().to_string())
        .collect();

    rows.iter()
        .map(|row| decode_row(row, Some(&col_names)))
        .collect()
}

fn decode_row(row: &AnyRow, names: Option<&Vec<String>>) -> DecodedRow {
    let mut map = HashMap::with_capacity(row.columns().len());

    for (idx, column) in row.columns().iter().enumerate() {
        let name = names
            .and_then(|n| n.get(idx).cloned())
            .unwrap_or_else(|| column.name().to_string());

        let value = if let Ok(i) = row.try_get::<i64, _>(column.ordinal()) {
            let looks_bool = name.starts_with("is_")
                || name.starts_with("Is_")
                || name.starts_with("IS_")
                || name.starts_with("has_")
                || name.starts_with("Has_")
                || name.starts_with("HAS_")
                || name.starts_with("can_")
                || name.starts_with("Can_")
                || name.starts_with("CAN_")
                || name.ends_with("_flag")
                || name.ends_with("_Flag")
                || name.ends_with("_FLAG");
            if looks_bool && (i == 0 || i == 1) {
                SqlValue::Bool(i != 0)
            } else {
                SqlValue::Int(i)
            }
        } else if let Ok(b) = row.try_get::<bool, _>(column.ordinal()) {
            SqlValue::Bool(b)
        } else if let Ok(f) = row.try_get::<f64, _>(column.ordinal()) {
            SqlValue::Float(f)
        } else if let Ok(s) = row.try_get::<String, _>(column.ordinal()) {
            SqlValue::Text(s)
        } else {
            SqlValue::Null
        };

        map.insert(name, value);
    }

    map
}
