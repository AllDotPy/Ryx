use std::sync::Arc;
use tokio::sync::Mutex;

use ryx_backend::transaction::set_current_transaction;
use ryx_backend::transaction::TransactionHandle as BackendTxHandle;
use ryx_common::RyxResult;

/// Run a closure inside a database transaction.
///
/// The transaction is set as the active transaction globally, so all
/// ORM queries inside the closure automatically use it.
///
/// ```ignore
/// use ryx_rs::transaction;
///
/// transaction(|tx| async move {
///     User::objects().filter("id", 1).delete().await?;
///     tx.commit().await?;
///     Ok(())
/// }).await?;
/// ```
pub async fn transaction<F, Fut, T>(f: F) -> RyxResult<T>
where
    F: Send + FnOnce(TransactionHandle) -> Fut,
    Fut: Send + Future<Output = RyxResult<T>>,
    T: Send + 'static,
{
    let backend_tx = BackendTxHandle::begin(None).await?;
    let handle = TransactionHandle {
        inner: Arc::new(Mutex::new(Some(backend_tx))),
    };

    // Set as the globally active transaction so all backend calls use it
    set_current_transaction(Some(handle.inner.clone()));

    let result = f(handle).await;

    // Clear the active transaction
    set_current_transaction(None);

    result
}

/// Handle passed to the transaction closure.
///
/// Wraps the backend transaction handle and provides commit/rollback.
pub struct TransactionHandle {
    pub(crate) inner: Arc<Mutex<Option<BackendTxHandle>>>,
}

impl TransactionHandle {
    /// Commit the transaction.
    pub async fn commit(&self) -> RyxResult<()> {
        let guard = self.inner.lock().await;
        if let Some(tx) = guard.as_ref() {
            tx.commit().await?;
        }
        Ok(())
    }

    /// Roll back the transaction.
    pub async fn rollback(&self) -> RyxResult<()> {
        let guard = self.inner.lock().await;
        if let Some(tx) = guard.as_ref() {
            tx.rollback().await?;
        }
        Ok(())
    }
}
