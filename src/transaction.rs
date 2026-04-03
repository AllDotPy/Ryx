//
// ###
// Ryx — Transaction Manager
//
// Provides a Rust-side transaction handle that:
//   - Acquires a connection from the pool
//   - Wraps it in a sqlx transaction (BEGIN on acquire)
//   - Exposes commit() and rollback() to Python
//   - Supports named SAVEPOINTs for nested transactions
//   - Exposes execute_in_tx() so SQL can run within the transaction boundary
//
// Design decision: we use sqlx::Transaction<sqlx::Any> so one code path
// handles Postgres, MySQL, and SQLite. The transaction is stored behind an
// Arc<Mutex<...>> so it can be sent across the PyO3 boundary and used from
// multiple Python await points without re-acquiring the GIL.
//
// Usage from Python (via ryx/transaction.py):
//   async with ryx.transaction() as tx:
//       await Post.objects.filter(pk=1).update(views=42)  # uses tx automatically
//       await tx.commit()   # optional — commits on __aexit__ by default
//
// Savepoints (nested transactions):
//   async with ryx.transaction() as tx:
//       sp = await tx.savepoint("sp1")
//       ...
//       await tx.rollback_to("sp1")
// ###

use once_cell::sync::OnceCell;
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::Mutex;

use sqlx::{Any, Transaction};
use tracing::{debug, instrument};

use crate::errors::{RyxError, RyxResult};
use crate::pool;
use crate::query::ast::SqlValue;
use crate::query::compiler::CompiledQuery;

static ACTIVE_TX: OnceCell<StdMutex<Option<Arc<Mutex<Option<TransactionHandle>>>>>> =
    OnceCell::new();

pub fn set_current_transaction(tx: Option<Arc<Mutex<Option<TransactionHandle>>>>) {
    let lock = ACTIVE_TX.get_or_init(|| StdMutex::new(None));
    let mut guard = lock.lock().unwrap();
    *guard = tx;
}

pub fn get_current_transaction() -> Option<Arc<Mutex<Option<TransactionHandle>>>> {
    let lock = ACTIVE_TX.get_or_init(|| StdMutex::new(None));
    lock.lock().unwrap().clone()
}

// ###
// TransactionHandle — owns a live sqlx Transaction
// ###

/// Wraps a live sqlx transaction.
///
/// The `Arc<Mutex<Option<Transaction>>>` pattern:
/// - `Arc`    → shared ownership so PyO3 can clone the handle
/// - `Mutex`  → interior mutability needed for commit/rollback (consume the tx)
/// - `Option` → lets us take() the transaction out on commit/rollback without
///              needing to return it afterwards (avoids use-after-free)
pub struct TransactionHandle {
    inner: Arc<Mutex<Option<Transaction<'static, Any>>>>,
    savepoints: Vec<String>,
}

impl TransactionHandle {
    /// Begin a new transaction by acquiring a connection from the pool.
    pub async fn begin() -> RyxResult<Self> {
        let pool = pool::get()?;
        debug!("Beginning transaction");
        let tx = pool.begin().await.map_err(RyxError::Database)?;

        Ok(Self {
            inner: Arc::new(Mutex::new(Some(tx))),
            savepoints: Vec::new(),
        })
    }

    /// Commit the transaction.
    ///
    /// After this call the transaction is consumed and the handle is invalid.
    /// Calling commit() or rollback() again on the same handle is a no-op
    /// (returns Ok without touching the DB).
    pub async fn commit(&self) -> RyxResult<()> {
        let mut guard = self.inner.lock().await;
        if let Some(tx) = guard.take() {
            debug!("Committing transaction");
            tx.commit().await.map_err(RyxError::Database)?;
        }
        Ok(())
    }

    /// Roll back the transaction.
    ///
    /// Same semantics as commit() — safe to call multiple times.
    pub async fn rollback(&self) -> RyxResult<()> {
        let mut guard = self.inner.lock().await;
        if let Some(tx) = guard.take() {
            debug!("Rolling back transaction");
            tx.rollback().await.map_err(RyxError::Database)?;
        }
        Ok(())
    }

    /// Create a named savepoint within the transaction.
    ///
    /// Savepoints allow partial rollback without aborting the entire transaction.
    /// The savepoint name must be a valid SQL identifier.
    pub async fn savepoint(&mut self, name: &str) -> RyxResult<()> {
        self.execute_raw(&format!("SAVEPOINT {name}")).await?;
        self.savepoints.push(name.to_string());
        debug!("Created savepoint: {name}");
        Ok(())
    }

    /// Roll back to a named savepoint.
    pub async fn rollback_to(&self, name: &str) -> RyxResult<()> {
        self.execute_raw(&format!("ROLLBACK TO SAVEPOINT {name}"))
            .await?;
        debug!("Rolled back to savepoint: {name}");
        Ok(())
    }

    /// Release (drop) a named savepoint.
    pub async fn release_savepoint(&self, name: &str) -> RyxResult<()> {
        self.execute_raw(&format!("RELEASE SAVEPOINT {name}"))
            .await?;
        Ok(())
    }

    /// Execute a pre-compiled query within this transaction.
    ///
    /// The query is run on the transaction's connection (not the pool), so it
    /// participates in the current transaction boundary.
    #[instrument(skip(self, query), fields(sql = %query.sql))]
    pub async fn execute_query(&self, query: CompiledQuery) -> RyxResult<u64> {
        let mut guard = self.inner.lock().await;
        let tx = guard.as_mut().ok_or_else(|| {
            RyxError::Internal("Transaction already committed or rolled back".into())
        })?;

        let mut q = sqlx::query(&query.sql);
        for value in &query.values {
            q = bind_value(q, value);
        }
        let result = q.execute(&mut **tx).await.map_err(RyxError::Database)?;
        Ok(result.rows_affected())
    }

    /// Execute a raw SQL string within this transaction (no bind params).
    async fn execute_raw(&self, sql: &str) -> RyxResult<()> {
        let mut guard = self.inner.lock().await;
        let tx = guard.as_mut().ok_or_else(|| {
            RyxError::Internal("Transaction already committed or rolled back".into())
        })?;
        sqlx::query(sql)
            .execute(&mut **tx)
            .await
            .map_err(RyxError::Database)?;
        Ok(())
    }

    /// Fetch rows within this transaction.
    pub async fn fetch_query(
        &self,
        query: CompiledQuery,
    ) -> RyxResult<Vec<std::collections::HashMap<String, serde_json::Value>>> {
        let mut guard = self.inner.lock().await;
        let tx = guard.as_mut().ok_or_else(|| {
            RyxError::Internal("Transaction already committed or rolled back".into())
        })?;

        let mut q = sqlx::query(&query.sql);
        for value in &query.values {
            q = bind_value(q, value);
        }

        use sqlx::{Column, Row};
        let rows = q.fetch_all(&mut **tx).await.map_err(RyxError::Database)?;

        Ok(rows
            .iter()
            .map(|row| {
                let mut map = std::collections::HashMap::new();
                for col in row.columns() {
                    let name = col.name().to_string();
                    let val: serde_json::Value =
                        if let Ok(b) = row.try_get::<bool, _>(col.ordinal()) {
                            serde_json::Value::Bool(b)
                        } else if let Ok(i) = row.try_get::<i64, _>(col.ordinal()) {
                            serde_json::Value::Number(i.into())
                        } else if let Ok(f) = row.try_get::<f64, _>(col.ordinal()) {
                            serde_json::Number::from_f64(f)
                                .map(serde_json::Value::Number)
                                .unwrap_or(serde_json::Value::Null)
                        } else if let Ok(s) = row.try_get::<String, _>(col.ordinal()) {
                            serde_json::Value::String(s)
                        } else {
                            serde_json::Value::Null
                        };
                    map.insert(name, val);
                }
                map
            })
            .collect())
    }

    /// Whether the transaction is still active (not yet committed or rolled back).
    pub async fn is_active(&self) -> bool {
        self.inner.lock().await.is_some()
    }
}

// Helper: bind a SqlValue to a sqlx query (mirrors executor.rs)
fn bind_value<'q>(
    q: sqlx::query::Query<'q, sqlx::Any, sqlx::any::AnyArguments<'q>>,
    value: &'q SqlValue,
) -> sqlx::query::Query<'q, sqlx::Any, sqlx::any::AnyArguments<'q>> {
    match value {
        SqlValue::Null => q.bind(None::<String>),
        SqlValue::Bool(b) => q.bind(*b),
        SqlValue::Int(i) => q.bind(*i),
        SqlValue::Float(f) => q.bind(*f),
        SqlValue::Text(s) => q.bind(s.as_str()),
        SqlValue::List(_) => {
            tracing::warn!("List value in transaction execute — compiler bug");
            q
        }
    }
}
