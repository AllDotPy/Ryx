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
// Design decision: we use RyxTransaction enum to handle Postgres, MySQL, and SQLite.
// The transaction is stored behind an Arc<Mutex<...>> so it can be sent across the PyO3 boundary.
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

use ryx_common::errors::{RyxError, RyxResult};
use ryx_query::compiler::CompiledQuery;

use crate::backends::{RowView, RyxBackend, RyxTransaction};
use crate::pool;

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
// TransactionHandle — owns a live RyxTransaction
// ###

/// Wraps a live sqlx transaction.
pub struct TransactionHandle {
    inner: Arc<Mutex<Option<RyxTransaction>>>,
    savepoints: Vec<String>,
    pub alias: Option<String>,
}

impl TransactionHandle {
    /// Begin a new transaction by acquiring a connection from the pool.
    pub async fn begin(alias: Option<String>) -> RyxResult<Self> {
        let pool_backend: Arc<dyn RyxBackend> = pool::get(alias.as_deref())?;
        let tx = pool_backend.get_pool().begin().await?;

        Ok(Self {
            inner: Arc::new(Mutex::new(Some(tx))),
            savepoints: Vec::new(),
            alias: alias.clone(),
        })
    }

    /// Commit the transaction.
    pub async fn commit(&self) -> RyxResult<()> {
        let mut guard = self.inner.lock().await;
        if let Some(tx) = guard.take() {
            match tx {
                RyxTransaction::Postgres(tx) => tx.commit().await.map_err(RyxError::Database),
                RyxTransaction::MySql(tx) => tx.commit().await.map_err(RyxError::Database),
                RyxTransaction::Sqlite(tx) => tx.commit().await.map_err(RyxError::Database),
            }?;
        }
        Ok(())
    }

    /// Roll back the transaction.
    pub async fn rollback(&self) -> RyxResult<()> {
        let mut guard = self.inner.lock().await;
        if let Some(tx) = guard.take() {
            match tx {
                RyxTransaction::Postgres(tx) => tx.rollback().await.map_err(RyxError::Database),
                RyxTransaction::MySql(tx) => tx.rollback().await.map_err(RyxError::Database),
                RyxTransaction::Sqlite(tx) => tx.rollback().await.map_err(RyxError::Database),
            }?;
        }
        Ok(())
    }

    /// Create a named savepoint within the transaction.
    pub async fn savepoint(&mut self, name: &str) -> RyxResult<()> {
        self.execute_raw(&format!("SAVEPOINT {name}")).await?;
        self.savepoints.push(name.to_string());
        Ok(())
    }

    /// Roll back to a named savepoint.
    pub async fn rollback_to(&self, name: &str) -> RyxResult<()> {
        self.execute_raw(&format!("ROLLBACK TO SAVEPOINT {name}"))
            .await?;
        Ok(())
    }

    /// Release (drop) a named savepoint.
    pub async fn release_savepoint(&self, name: &str) -> RyxResult<()> {
        self.execute_raw(&format!("RELEASE SAVEPOINT {name}"))
            .await?;
        Ok(())
    }

    /// Execute a pre-compiled query within this transaction.
    pub async fn execute_query(&self, query: CompiledQuery) -> RyxResult<u64> {
        let mut guard = self.inner.lock().await;
        let tx = guard.as_mut().ok_or_else(|| {
            RyxError::Internal("Transaction already committed or rolled back".into())
        })?;
        tx.execute_query(query).await
    }

    /// Execute a raw SQL string within this transaction.
    async fn execute_raw(&self, sql: &str) -> RyxResult<()> {
        let mut guard = self.inner.lock().await;
        let tx = guard.as_mut().ok_or_else(|| {
            RyxError::Internal("Transaction already committed or rolled back".into())
        })?;
        tx.execute_raw(sql).await
    }

    /// Fetch rows within this transaction.
    pub async fn fetch_query(&self, query: CompiledQuery) -> RyxResult<Vec<RowView>> {
        let mut guard = self.inner.lock().await;
        let tx = guard.as_mut().ok_or_else(|| {
            RyxError::Internal("Transaction already committed or rolled back".into())
        })?;
        tx.fetch_query(query).await
    }

    /// Whether the transaction is still active.
    pub async fn is_active(&self) -> bool {
        self.inner.lock().await.is_some()
    }
}
