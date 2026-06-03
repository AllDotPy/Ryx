pub mod agg;
pub mod cache;
pub mod config;
pub mod into_sql;
pub mod migration;
pub mod model;
pub mod objects;
pub mod q;
pub mod queryset;
pub mod row;
pub mod stream;
pub mod transaction;

use std::sync::OnceLock;

// Re-export key traits and types for convenience
pub use config::{is_initialized, RyxConfig, PoolConfigSection, MigrationsConfig};
pub use model::{FieldMeta, Model, RelationMeta, Relationships};
pub use objects::{InsertBuilder, ObjectsManager};
pub use q::Q;
pub use queryset::QuerySet;
pub use row::FromRow;
pub use transaction::transaction;

/// Initialize the global `tracing` subscriber from `RYX_LOG_LEVEL`.
///
/// Called automatically by [`init()`]. Safe to call multiple times —
/// only the first call takes effect.
pub fn init_tracing() {
    static INIT: OnceLock<()> = OnceLock::new();
    INIT.get_or_init(|| {
        let level =
            std::env::var("RYX_LOG_LEVEL").unwrap_or_default();
        if level.is_empty() {
            return;
        }
        let filter = format!("ryx={}", level.to_lowercase());
        let _ = tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(true)
            .try_init();
    });
}

/// Auto-detect config and initialize the pool.
///
/// Equivalent to the Python `import ryx` auto-setup:
/// 1. Loads `ryx.yaml` → `ryx.yml` → `ryx.toml` → `ryx.json`
/// 2. Applies env vars (`RYX_DATABASE_URL`, `RYX_DB_<ALIAS>_URL`, `RYX_POOL_*`)
/// 3. Initializes the global database pool
///
/// No-op if already initialized or no config is found.
pub async fn init() -> RyxResult<()> {
    init_tracing();
    tracing::info!("Initializing Ryx ...");
    config::init().await
}

// Re-export common types
pub use ryx_common::PoolConfig;
pub use ryx_common::RyxError;
pub use ryx_common::RyxResult;
pub use ryx_common::SqlValue;

// Re-export the derive macros
pub use ryx_macro::{FromRow, Model};

// Re-export the #[model] attribute macro
pub use ryx_macro::model;

// Re-export serde so users don't need it as a direct dependency
pub use serde;
