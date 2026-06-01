//
// ###
// Ryx — Global Connection Pool
// ###
//
// Design decision: we maintain a single, global connection pool per process,
// stored in a `OnceLock<AnyPool>`. This mirrors how Django's database layer
// works: one connection pool per database, initialized once at startup.
//
// Why AnyPool instead of PgPool/MySqlPool/SqlitePool?
//   Using `sqlx::any::AnyPool` lets us support multiple backends with a single
//   code path. The trade-off is that we lose compile-time query checking (the
//   `query!` macro), but since we're building a dynamic ORM that constructs SQL
//   at runtime anyway, this is exactly the right trade-off.
//
// Initialization flow:
//   1. Python calls `await ryx.setup(url="postgres://...")`
//   2. That calls `pool::initialize(url, options)` from Rust
//   3. We build the pool and store it in POOL
//   4. All subsequent ORM calls retrieve the pool with `pool::get()`
//
// Thread safety:
//   `OnceLock` guarantees that initialization happens exactly once even if
//   multiple threads race to call `setup()`. Subsequent reads are lock-free.
// ###

use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};

use sqlx::{any::install_default_drivers, mysql::MySqlPool, postgres::PgPool, sqlite::SqlitePool};
use tracing::{debug, info};

use ryx_query::Backend;

use crate::backends::{
    RyxBackend, mysql::MySqlBackend, postgres::PostgresBackend, sqlite::SqliteBackend,
};
use ryx_common::errors::{RyxError, RyxResult};
pub use ryx_common::PoolConfig;

fn to_static<T: sqlx::Database>(tx: sqlx::Transaction<'_, T>) -> sqlx::Transaction<'static, T> {
    // SAFETY: transactions are tied to the process-lifetime pool. Extending the
    // lifetime lets us store them behind Arc<Mutex<..>> across the FFI
    // boundary without leaking the underlying connection.
    unsafe { std::mem::transmute::<sqlx::Transaction<'_, T>, sqlx::Transaction<'static, T>>(tx) }
}

/// Enum to represent the type of database backend Pools.
pub enum RyxPool {
    Postgres(PgPool),
    MySQL(MySqlPool),
    SQLite(SqlitePool),
}

impl RyxPool {
    pub async fn begin(&self) -> RyxResult<crate::backends::RyxTransaction> {
        match self {
            RyxPool::Postgres(pool) => {
                let tx = pool.begin().await.map_err(RyxError::Database)?;
                Ok(crate::backends::RyxTransaction::Postgres(to_static(tx)))
            }
            RyxPool::MySQL(pool) => {
                let tx = pool.begin().await.map_err(RyxError::Database)?;
                Ok(crate::backends::RyxTransaction::MySql(to_static(tx)))
            }
            RyxPool::SQLite(pool) => {
                let tx = pool.begin().await.map_err(RyxError::Database)?;
                Ok(crate::backends::RyxTransaction::Sqlite(to_static(tx)))
            }
        }
    }
}

/// A registry of database connection pools.
/// Allows multiple databases to be configured and accessed via aliases.
pub struct PoolRegistry {
    /// Map of alias (e.g., "default", "replica") to the connection pool and its backend.
    pub backends: HashMap<String, (Arc<dyn RyxBackend>, Backend)>,
    /// The alias used when no specific database is requested.
    pub default_alias: String,
}

/// Global singleton for the pool registry.
static REGISTRY: OnceLock<RwLock<PoolRegistry>> = OnceLock::new();

//
// Public API
//
/// Initialize the global connection pool registry.
///
/// # Arguments
/// * `database_urls` — a map of aliases to database URLs.
///   Example: `{"default": "postgres://...", "logs": "sqlite://..."}`
/// * `config` — pool tuning parameters (see [`PoolConfig`])
///
/// # Errors
/// - [`RyxError::PoolAlreadyInitialized`] if called more than once
/// - [`RyxError::Database`] if any URL is invalid or DB is unreachable
pub async fn initialize(
    database_urls: HashMap<String, String>,
    config: PoolConfig,
) -> RyxResult<()> {
    // Register all built-in sqlx drivers with AnyPool.
    install_default_drivers();

    if database_urls.is_empty() {
        return Err(RyxError::Internal(
            "No database URLs provided for initialization".into(),
        ));
    }

    debug!(urls = ?database_urls, "Initializing Ryx connection pool registry");

    let mut backends = HashMap::new();
    let mut first_alias = None;

    for (alias, url) in database_urls {
        if first_alias.is_none() {
            first_alias = Some(alias.clone());
        }
        // config.url = Some(url.clone());

        let db_backend = ryx_query::backend::detect_backend(&url);

        // Create a backend specified pool with the provided configuration.
        let ryx_backend: (Arc<dyn RyxBackend>, Backend) = match db_backend {
            Backend::PostgreSQL => {
                let b = PostgresBackend::new(config.clone(), url.clone()).await;
                (Arc::new(b), db_backend)
            }
            Backend::MySQL => {
                let b = MySqlBackend::new(config.clone(), url.clone()).await;
                (Arc::new(b), db_backend)
            }
            Backend::SQLite => {
                let b = SqliteBackend::new(config.clone(), url.clone()).await;
                (Arc::new(b), db_backend)
            }
        };

        backends.insert(alias, ryx_backend);
    }

    // Determine the default alias
    let default_alias = if backends.contains_key("default") {
        "default".to_string()
    } else {
        first_alias.expect("Registry cannot be empty")
    };

    let registry = PoolRegistry {
        backends,
        default_alias,
    };

    REGISTRY
        .set(RwLock::new(registry))
        .map_err(|_| RyxError::PoolAlreadyInitialized)?;

    info!("Ryx connection pool registry initialized successfully");
    Ok(())
}

/// Retrieve a reference to a specific connection pool.
///
/// # Arguments
/// * `alias` — the pool alias to retrieve. If `None`, the default pool is used.
///
/// # Errors
/// Returns [`RyxError::PoolNotInitialized`] if `initialize()` has not been called,
/// or if the specified alias does not exist.
pub fn get(alias: Option<&str>) -> RyxResult<Arc<dyn RyxBackend>> {
    let registry_lock = REGISTRY.get().ok_or(RyxError::PoolNotInitialized)?;
    let registry = registry_lock.read().unwrap();

    let target_alias = alias.unwrap_or(&registry.default_alias);

    registry
        .backends
        .get(target_alias)
        .map(|(b, _)| b.clone())
        .ok_or_else(|| RyxError::Internal(format!("Database pool '{}' not found", target_alias)))
}

/// Check whether the pool registry has been initialized.
pub fn is_initialized(alias: Option<String>) -> bool {
    // Alias provided
    if alias.is_some() {
        REGISTRY.get().is_some_and(|f| {
            f.read()
                .is_ok_and(|pc| pc.backends.contains_key(alias.unwrap().as_str()))
        })
    }
    // Else is the registry not none?
    else {
        REGISTRY.get().is_some()
    }
}

/// Return a list of all configured database aliases.
pub fn list_aliases() -> RyxResult<Vec<String>> {
    let registry_lock = REGISTRY.get().ok_or(RyxError::PoolNotInitialized)?;
    let registry = registry_lock.read().unwrap();
    Ok(registry.backends.keys().cloned().collect())
}

/// Retrieve the backend type for a specific pool.
///
/// # Errors
/// Returns [`RyxError::PoolNotInitialized`] if the registry is not set up,
/// or if the specified alias does not exist.
pub fn get_backend(alias: Option<&str>) -> RyxResult<Backend> {
    let registry_lock = REGISTRY.get().ok_or(RyxError::PoolNotInitialized)?;
    let registry = registry_lock.read().unwrap();

    let target_alias = alias.unwrap_or(&registry.default_alias);

    registry
        .backends
        .get(target_alias)
        .map(|(_, backend)| *backend)
        .ok_or_else(|| RyxError::Internal(format!("Database pool '{}' not found", target_alias)))
}

/// Return pool statistics for a specific pool.
#[derive(Debug)]
pub struct PoolStats {
    pub size: u32,
    pub idle: u32,
}

/// Retrieve current pool statistics for a specific pool.
pub fn stats(alias: Option<&str>) -> RyxResult<PoolStats> {
    let backend: Arc<dyn RyxBackend> = get(alias)?;
    Ok(backend.pool_stats())
}
