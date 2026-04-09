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
 
use serde::{Deserialize, Serialize};
use sqlx::{
    AnyPool,
    any::{AnyPoolOptions, install_default_drivers},
};
use tracing::{debug, info};
 
use crate::errors::{RyxError, RyxResult};
use ryx_query::Backend;

/// A registry of database connection pools.
/// Allows multiple databases to be configured and accessed via aliases.
pub struct PoolRegistry {
    /// Map of alias (e.g., "default", "replica") to the connection pool and its backend.
    pub pools: HashMap<String, (Arc<AnyPool>, Backend)>,
    /// The alias used when no specific database is requested.
    pub default_alias: String,
}

/// Global singleton for the pool registry.
static REGISTRY: OnceLock<RwLock<PoolRegistry>> = OnceLock::new();


// ###
// Pool configuration options
//
// We expose a subset of sqlx's PoolOptions to Python so users can tune the
// pool without having to write Rust. These map 1:1 to sqlx fields.
// ###

/// Configuration options for the connection pool.
///
/// Passed from Python to `initialize()`. All fields are optional — sane
/// defaults are applied when fields are `None`.
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Maximum number of connections the pool will maintain.
    /// Default: 10. Tune based on your database's `max_connections` setting.
    pub max_connections: u32,

    /// Minimum number of idle connections the pool will keep alive.
    /// Default: 1. Setting this higher reduces connection establishment latency
    /// at the cost of holding connections open.
    pub min_connections: u32,

    /// How long (in seconds) to wait for a connection before giving up.
    /// Default: 30s. Raise this for slow networks or cold-start scenarios.
    pub connect_timeout_secs: u64,

    /// How long (in seconds) an idle connection is kept before being closed.
    /// Default: 600s (10 min). Lower this if your database has a tight
    /// `wait_timeout` setting (common with MySQL/MariaDB).
    pub idle_timeout_secs: u64,

    /// Maximum lifetime (in seconds) of any connection regardless of usage.
    /// Default: 1800s (30 min). Protects against stale connections.
    pub max_lifetime_secs: u64,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_connections: 10,
            min_connections: 1,
            connect_timeout_secs: 30,
            idle_timeout_secs: 600,
            max_lifetime_secs: 1800,
        }
    }
}

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
pub async fn initialize(database_urls: HashMap<String, String>, config: PoolConfig) -> RyxResult<()> {
    // Register all built-in sqlx drivers with AnyPool.
    install_default_drivers();
 
    if database_urls.is_empty() {
        return Err(RyxError::Internal("No database URLs provided for initialization".into()));
    }

    debug!(urls = ?database_urls, "Initializing Ryx connection pool registry");
 
    let mut pools = HashMap::new();
    let mut first_alias = None;
 
    for (alias, url) in database_urls {
        if first_alias.is_none() {
            first_alias = Some(alias.clone());
        }
 
        let pool = AnyPoolOptions::new()
            .max_connections(config.max_connections)
            .min_connections(config.min_connections)
            .acquire_timeout(std::time::Duration::from_secs(config.connect_timeout_secs))
            .idle_timeout(std::time::Duration::from_secs(config.idle_timeout_secs))
            .max_lifetime(std::time::Duration::from_secs(config.max_lifetime_secs))
            .connect(&url)
            .await
            .map_err(RyxError::Database)?;
 
        let backend = ryx_query::backend::detect_backend(&url);
        pools.insert(alias, (Arc::new(pool), backend));
    }
 
    // Determine the default alias
    let default_alias = if pools.contains_key("default") {
        "default".to_string()
    } else {
        first_alias.expect("Registry cannot be empty")
    };
 
    let registry = PoolRegistry {
        pools,
        default_alias,
    };
 
    REGISTRY.set(RwLock::new(registry))
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
pub fn get(alias: Option<&str>) -> RyxResult<Arc<AnyPool>> {
    let registry_lock = REGISTRY.get().ok_or(RyxError::PoolNotInitialized)?;
    let registry = registry_lock.read().unwrap();
 
    let target_alias = alias.unwrap_or(&registry.default_alias);
    
    registry.pools.get(target_alias)
        .map(|(pool, _)| pool.clone())
        .ok_or_else(|| RyxError::Internal(format!("Database pool '{}' not found", target_alias)))
}
 
/// Check whether the pool registry has been initialized.
pub fn is_initialized(alias: Option<String>) -> bool {
    
    // Alias provided
    if alias.is_some(){
        REGISTRY.get().is_some_and(|f| {
            f.read().is_ok_and(|pc| pc.pools.contains_key(alias.unwrap().as_str()))
        })
    }
    // Else is the registry not none?
    else {
        REGISTRY.get().is_some()
    }
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
    
    registry.pools.get(target_alias)
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
    let pool = get(alias)?;
    Ok(PoolStats {
        size: pool.size(),
        idle: pool.num_idle() as u32,
    })
}

