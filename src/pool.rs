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

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use sqlx::{
    AnyPool,
    any::{AnyPoolOptions, install_default_drivers},
};
use tracing::{debug, info};

use crate::errors::{RyxError, RyxResult};
use ryx_query::Backend;

// ###
// Global singleton
//
// We use `std::sync::OnceLock` (stable since Rust 1.70) rather than
// `once_cell::sync::OnceCell` to avoid an extra dependency for this specific
// use case. OnceLock is conceptually identical.
// ###

/// The single global connection pool for this process.
///
/// Initialized exactly once by `initialize()`. All ORM operations retrieve
/// the pool via `get()`.
static POOL: OnceLock<AnyPool> = OnceLock::new();

/// The backend type for the initialized pool.
/// Set at initialization time based on the database URL.
static BACKEND: OnceLock<Backend> = OnceLock::new();

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

/// Initialize the global connection pool.
///
/// # Arguments
/// * `database_url` — a standard database URL, e.g.:
///   - `"postgres://user:pass@localhost/dbname"`
///   - `"mysql://user:pass@localhost/dbname"`
///   - `"sqlite:///path/to/db.sqlite3"` or `"sqlite::memory:"`
/// * `config` — optional pool tuning parameters (see [`PoolConfig`])
///
/// # Errors
/// - [`RyxError::PoolAlreadyInitialized`] if called more than once
/// - [`RyxError::Database`] if the URL is invalid or the DB is unreachable
///
/// # Design note
/// We call `install_default_drivers()` here. This registers the Postgres,
/// MySQL, and SQLite drivers with sqlx's `AnyPool` machinery. Without this
/// call, `AnyPool::connect()` panics with "no driver for scheme". The call
/// is idempotent so it's safe to call multiple times (though we only ever
/// call it once via OnceLock).
pub async fn initialize(database_url: &str, config: PoolConfig) -> RyxResult<()> {
    // Register all built-in sqlx drivers with AnyPool.
    // This must be called before any AnyPool operation.
    install_default_drivers();

    debug!(url = %database_url, "Initializing Ryx connection pool");

    let pool = AnyPoolOptions::new()
        .max_connections(config.max_connections)
        .min_connections(config.min_connections)
        .acquire_timeout(std::time::Duration::from_secs(config.connect_timeout_secs))
        .idle_timeout(std::time::Duration::from_secs(config.idle_timeout_secs))
        .max_lifetime(std::time::Duration::from_secs(config.max_lifetime_secs))
        .connect(database_url)
        .await
        .map_err(RyxError::Database)?;

    // OnceLock::set returns Err(value) if already set.
    // We return our own error type to give a clearer message to users.
    POOL.set(pool)
        .map_err(|_| RyxError::PoolAlreadyInitialized)?;

    // Set the backend type based on the URL
    let backend = ryx_query::backend::detect_backend(database_url);
    BACKEND.set(backend).ok();

    info!("Ryx connection pool initialized successfully");
    Ok(())
}

/// Retrieve a reference to the global connection pool.
///
/// # Errors
/// Returns [`RyxError::PoolNotInitialized`] if `initialize()` has not been
/// called. Every ORM operation calls this first, so users get a clear error
/// message rather than a panic.
pub fn get() -> RyxResult<&'static AnyPool> {
    POOL.get().ok_or(RyxError::PoolNotInitialized)
}

/// Check whether the pool has been initialized without consuming it.
/// Useful for diagnostic / health-check endpoints.
pub fn is_initialized() -> bool {
    POOL.get().is_some()
}

/// Retrieve the current backend type.
///
/// # Errors
/// Returns [`RyxError::PoolNotInitialized`] if `initialize()` has not been called.
pub fn get_backend() -> RyxResult<Backend> {
    BACKEND.get().copied().ok_or(RyxError::PoolNotInitialized)
}

/// Return pool statistics as a simple struct.
/// Exposed to Python for monitoring and debugging.
#[derive(Debug)]
pub struct PoolStats {
    pub size: u32,
    pub idle: u32,
}

/// Retrieve current pool statistics.
///
/// # Errors
/// Returns [`RyxError::PoolNotInitialized`] if the pool is not yet set up.
pub fn stats() -> RyxResult<PoolStats> {
    let pool = get()?;
    Ok(PoolStats {
        size: pool.size(),
        idle: pool.num_idle() as u32,
    })
}
