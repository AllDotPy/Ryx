use std::collections::HashMap;
use std::path::Path;

use ryx_common::RyxResult;

/// Full Ryx configuration, loadable from config files or env vars.
///
/// Search order (first found wins): `ryx.yaml` → `ryx.yml` → `ryx.toml` → `ryx.json`
///
/// Environment variables fill in gaps not present in the config file
/// (config file values take precedence, matching Python `_auto_setup()`):
/// - `RYX_DATABASE_URL` → `urls.default`
/// - `RYX_DB_<ALIAS>_URL` → `urls.<alias>`  (e.g. `RYX_DB_LOGS_URL` → `urls.logs`)
/// - `RYX_POOL_MAX_CONNECTIONS` → `pool.max_conn`
/// - `RYX_POOL_MIN_CONNECTIONS` → `pool.min_conn`
/// - `RYX_POOL_CONNECT_TIMEOUT` → `pool.connect_timeout`
/// - `RYX_POOL_IDLE_TIMEOUT` → `pool.idle_timeout`
/// - `RYX_POOL_MAX_LIFETIME` → `pool.max_lifetime`
///
/// ```ignore
/// # ryx.toml
/// [urls]
/// default = "sqlite::memory:"
/// replica = "postgres://user:pass@host/db"
///
/// [pool]
/// max_conn = 12
/// min_conn = 2
/// connect_timeout = 30
///
/// [migrations]
/// dirs = ["migrations/"]
/// format = "YAML"
/// ```
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RyxConfig {
    /// Map of alias → database URL.
    #[serde(default)]
    pub urls: HashMap<String, String>,

    /// Connection pool settings.
    #[serde(default)]
    pub pool: PoolConfigSection,

    /// Migration settings.
    #[serde(default)]
    pub migrations: MigrationsConfig,
}

/// Pool configuration section — mirrors the Python `[pool]` block.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PoolConfigSection {
    #[serde(default)]
    pub max_conn: Option<u32>,
    #[serde(default)]
    pub min_conn: Option<u32>,
    #[serde(default)]
    pub connect_timeout: Option<u64>,
    #[serde(default)]
    pub idle_timeout: Option<u64>,
    #[serde(default)]
    pub max_lifetime: Option<u64>,
}

impl Default for PoolConfigSection {
    fn default() -> Self {
        Self {
            max_conn: None,
            min_conn: None,
            connect_timeout: None,
            idle_timeout: None,
            max_lifetime: None,
        }
    }
}

/// Migrations configuration section.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MigrationsConfig {
    /// Directories where migration files are stored.
    #[serde(default)]
    pub dirs: Vec<String>,
    /// Format of migration files: "YAML" or "TOML".
    pub format: Option<String>,
}

impl Default for MigrationsConfig {
    fn default() -> Self {
        Self {
            dirs: vec!["migrations/".into()],
            format: Some("YAML".into()),
        }
    }
}

impl Default for RyxConfig {
    fn default() -> Self {
        Self {
            urls: HashMap::new(),
            pool: PoolConfigSection::default(),
            migrations: MigrationsConfig::default(),
        }
    }
}

impl RyxConfig {
    /// Load config from the current directory:
    /// 1. Search `ryx.yaml` → `ryx.yml` → `ryx.toml` → `ryx.json`
    /// 2. Apply environment variable overrides
    pub fn load() -> Self {
        Self::load_from_dir(".")
    }

    /// Load config from a specific directory, then apply env overrides.
    pub fn load_from_dir(dir: &str) -> Self {
        let mut config = Self::load_file(dir);
        config.apply_env_overrides();
        config
    }

    fn load_file(dir: &str) -> Self {
        let candidates: [(&str, Option<&str>); 4] = [
            ("ryx.yaml", Some("yaml")),
            ("ryx.yml", Some("yaml")),
            ("ryx.toml", Some("toml")),
            ("ryx.json", Some("json")),
        ];

        for (filename, fmt) in &candidates {
            let path = Path::new(dir).join(filename);

            if !path.exists() {
                continue;
            }
            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            match *fmt {
                #[cfg(feature = "config-toml")]
                Some("toml") => {
                    if let Ok(cfg) = toml::from_str(&content) {
                        return cfg;
                    }
                }
                #[cfg(feature = "config-yaml")]
                Some("yaml") => {
                    if let Ok(cfg) = serde_yaml::from_str(&content) {
                        return cfg;
                    }
                }
                Some("json") => {
                    if let Ok(cfg) = serde_json::from_str(&content) {
                        return cfg;
                    }
                }
                _ => {}
            }
        }

        Self::default()
    }

    /// Override config values from environment variables.
    ///
    /// Matches the Python env-var convention exactly:
    /// - `RYX_DATABASE_URL` → `urls.default` (only if not already set by file)
    /// - `RYX_DB_<ALIAS>_URL` → `urls.<alias>` (only if not already set by file)
    /// - `RYX_POOL_*` → pool settings (only if not already set by file)
    ///
    /// **Precedence**: config file values take priority over env vars,
    /// matching the Python [`_auto_setup()`] behavior where config file
    /// URLs are `.update()`'d after env URLs.
    pub fn apply_env_overrides(&mut self) {
        // Per-alias URLs: RYX_DB_<ALIAS>_URL (env fills gaps only)
        for (key, value) in std::env::vars() {
            if let Some(alias) = key
                .strip_prefix("RYX_DB_")
                .and_then(|rest| rest.strip_suffix("_URL"))
            {
                let alias = alias.to_lowercase();
                self.urls.entry(alias).or_insert(value);
            }
        }

        // Default URL (env fills gap if no file/default URL)
        if let Ok(url) = std::env::var("RYX_DATABASE_URL") {
            self.urls.entry("default".into()).or_insert(url);
        }

        // Pool settings (env fills gaps only)
        if let Ok(v) = std::env::var("RYX_POOL_MAX_CONNECTIONS") {
            if let Ok(n) = v.parse() {
                self.pool.max_conn = self.pool.max_conn.or(Some(n));
            }
        }
        if let Ok(v) = std::env::var("RYX_POOL_MIN_CONNECTIONS") {
            if let Ok(n) = v.parse() {
                self.pool.min_conn = self.pool.min_conn.or(Some(n));
            }
        }
        if let Ok(v) = std::env::var("RYX_POOL_CONNECT_TIMEOUT") {
            if let Ok(n) = v.parse() {
                self.pool.connect_timeout = self.pool.connect_timeout.or(Some(n));
            }
        }
        if let Ok(v) = std::env::var("RYX_POOL_IDLE_TIMEOUT") {
            if let Ok(n) = v.parse() {
                self.pool.idle_timeout = self.pool.idle_timeout.or(Some(n));
            }
        }
        if let Ok(v) = std::env::var("RYX_POOL_MAX_LIFETIME") {
            if let Ok(n) = v.parse() {
                self.pool.max_lifetime = self.pool.max_lifetime.or(Some(n));
            }
        }
    }

    /// Build a `PoolConfig` and initialize the global database pool.
    ///
    /// Equivalent to calling `ryx.setup(urls, ...)` from Python.
    pub async fn init_pool(&self) -> RyxResult<()> {
        let pool_config = ryx_common::PoolConfig {
            max_connections: self.pool.max_conn.unwrap_or(10),
            min_connections: self.pool.min_conn.unwrap_or(1),
            connect_timeout_secs: self.pool.connect_timeout.unwrap_or(30),
            idle_timeout_secs: self.pool.idle_timeout.unwrap_or(600),
            max_lifetime_secs: self.pool.max_lifetime.unwrap_or(1800),
        };
        ryx_backend::pool::initialize(self.urls.clone(), pool_config).await
    }
}

/// Check whether the global pool has been initialized.
pub fn is_initialized() -> bool {
    ryx_backend::pool::list_aliases().is_ok()
}

/// Auto-detect configuration and initialize the pool.
///
/// Equivalent to the Python auto-setup that runs on `import ryx`:
/// 1. Search for `ryx.yaml` → `ryx.yml` → `ryx.toml` → `ryx.json` in CWD
/// 2. Apply `RYX_DATABASE_URL` / `RYX_DB_<ALIAS>_URL` / `RYX_POOL_*` env vars
///    (file values take precedence over env vars, matching Python)
/// 3. Initialize the global database pool
///
/// Returns `Ok(())` even if no config is found (no-op). Errors only on
/// pool initialization failure.
pub async fn init() -> RyxResult<()> {
    if is_initialized() {
        return Ok(());
    }
    let config = RyxConfig::load();
    // If no URLs are configured, this is a no-op — the user must call
    // `RyxConfig::load().init_pool().await` or `ryx_rs::setup()` manually.
    if config.urls.is_empty() {
        return Ok(());
    }
    config.init_pool().await
}
