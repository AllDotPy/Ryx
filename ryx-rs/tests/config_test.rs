use std::collections::HashMap;
use std::sync::Mutex;

/// Serialise tests that touch environment variables.
static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Test RyxConfig loading from TOML string (simulates ryx.toml).
#[test]
fn test_config_from_toml() {
    let toml_str = r#"
[urls]
default = "sqlite::memory:"
replica = "postgres://user:pass@localhost:5432/db"
logs = "sqlite:///tmp/logs.db"

[pool]
max_conn = 12
min_conn = 2
connect_timeout = 15

[migrations]
dirs = ["db/migrations/"]
format = "YAML"
"#;

    let config: ryx_rs::RyxConfig = toml::from_str(toml_str).expect("TOML parse");
    assert_eq!(config.urls.len(), 3);
    assert_eq!(config.urls.get("default").unwrap(), "sqlite::memory:");
    assert_eq!(config.urls.get("replica").unwrap(), "postgres://user:pass@localhost:5432/db");
    assert_eq!(config.urls.get("logs").unwrap(), "sqlite:///tmp/logs.db");

    assert_eq!(config.pool.max_conn, Some(12));
    assert_eq!(config.pool.min_conn, Some(2));
    assert_eq!(config.pool.connect_timeout, Some(15));
    assert_eq!(config.pool.idle_timeout, None); // not specified → None (not default)
    assert_eq!(config.pool.max_lifetime, None);  // not specified → None

    assert_eq!(config.migrations.dirs, vec!["db/migrations/"]);
    assert_eq!(config.migrations.format.as_deref(), Some("YAML"));
}

/// Test RyxConfig loading from YAML.
#[test]
fn test_config_from_yaml() {
    let yaml_str = r#"
urls:
  default: "sqlite::memory:"
  replica: "postgres://user:pass@localhost:5432/db"

pool:
  max_conn: 8
  min_conn: 1

migrations:
  dirs:
    - "migrations/"
"#;

    let config: ryx_rs::RyxConfig = serde_yaml::from_str(yaml_str).expect("YAML parse");
    assert_eq!(config.urls.len(), 2);
    assert_eq!(config.pool.max_conn, Some(8));
    assert_eq!(config.pool.min_conn, Some(1));
    assert_eq!(config.migrations.dirs, vec!["migrations/"]);
    assert_eq!(config.migrations.format.as_deref(), None);
}

/// Test defaults via `RyxConfig::default()`.
#[test]
fn test_config_defaults() {
    let config = ryx_rs::RyxConfig::default();
    assert!(config.urls.is_empty());
    // Pool defaults are None; real defaults are resolved in init_pool()
    assert_eq!(config.pool.max_conn, None);
    assert_eq!(config.pool.min_conn, None);
    assert_eq!(config.pool.connect_timeout, None);
    assert_eq!(config.pool.idle_timeout, None);
    assert_eq!(config.pool.max_lifetime, None);
    assert_eq!(config.migrations.dirs, vec!["migrations/"]);
    assert_eq!(config.migrations.format.as_deref(), Some("YAML"));
}

/// Test loading from ryx.toml file.
#[test]
fn test_config_load_from_file() {
    let _lock = ENV_LOCK.lock().unwrap();
    let tmp = std::env::temp_dir().join("ryx_test_config_load");
    let _ = std::fs::create_dir_all(&tmp);

    std::fs::write(
        tmp.join("ryx.toml"),
        r#"
[urls]
default = "sqlite::memory:"

[pool]
max_conn = 5
"#,
    )
    .expect("write test config");

    let dir = tmp.to_str().expect("utf-8 temp dir");
    let config = ryx_rs::RyxConfig::load_from_dir(dir);
    assert_eq!(config.urls.get("default").unwrap(), "sqlite::memory:");
    assert_eq!(config.pool.max_conn, Some(5));

    let _ = std::fs::remove_dir_all(&tmp);
}

/// Test env var overrides with Python-compatible variable names.
#[test]
fn test_config_env_overrides() {
    let _lock = ENV_LOCK.lock().unwrap();
    // Save previous values
    let old_default = std::env::var("RYX_DATABASE_URL").ok();
    let old_logs = std::env::var("RYX_DB_LOGS_URL").ok();
    let old_replica = std::env::var("RYX_DB_REPLICA_URL").ok();
    let old_max_conn = std::env::var("RYX_POOL_MAX_CONNECTIONS").ok();
    let old_idle = std::env::var("RYX_POOL_IDLE_TIMEOUT").ok();

    // Set test env vars (unsafe in edition 2024)
    unsafe {
        std::env::set_var("RYX_DATABASE_URL", "sqlite:///env_default.db");
        std::env::set_var("RYX_DB_LOGS_URL", "sqlite:///env_logs.db");
        std::env::set_var("RYX_DB_REPLICA_URL", "postgres://env_replica/db");
        std::env::set_var("RYX_POOL_MAX_CONNECTIONS", "20");
        std::env::set_var("RYX_POOL_IDLE_TIMEOUT", "300");
    }

    let config = ryx_rs::RyxConfig::load();

    // Default URL — env fills gap since no file/default URL exists
    assert_eq!(config.urls.get("default").unwrap(), "sqlite:///env_default.db");

    // Per-alias URLs (Python convention: RYX_DB_<ALIAS>_URL)
    assert_eq!(config.urls.get("logs").unwrap(), "sqlite:///env_logs.db");
    assert_eq!(config.urls.get("replica").unwrap(), "postgres://env_replica/db");

    // Pool overrides (defaults are None; env fills gaps)
    assert_eq!(config.pool.max_conn, Some(20));
    assert_eq!(config.pool.idle_timeout, Some(300));

    // Unset pool fields remain None (env didn't set them)
    assert_eq!(config.pool.min_conn, None);
    assert_eq!(config.pool.connect_timeout, None);

    // Restore
    set_or_remove("RYX_DATABASE_URL", old_default);
    set_or_remove("RYX_DB_LOGS_URL", old_logs);
    set_or_remove("RYX_DB_REPLICA_URL", old_replica);
    set_or_remove("RYX_POOL_MAX_CONNECTIONS", old_max_conn);
    set_or_remove("RYX_POOL_IDLE_TIMEOUT", old_idle);
}

#[test]
fn test_config_env_overrides_file() {
    let _lock = ENV_LOCK.lock().unwrap();
    let tmp = std::env::temp_dir().join("ryx_test_env_override");
    let _ = std::fs::create_dir_all(&tmp);

    std::fs::write(
        tmp.join("ryx.toml"),
        r#"
[urls]
default = "sqlite:///file_default.db"
logs = "sqlite:///file_logs.db"

[pool]
max_conn = 5
"#,
    )
    .expect("write test config");

    let dir = tmp.to_str().expect("utf-8");

    // Save + set
    let old_default = std::env::var("RYX_DATABASE_URL").ok();
    let old_logs = std::env::var("RYX_DB_LOGS_URL").ok();
    let old_max = std::env::var("RYX_POOL_MAX_CONNECTIONS").ok();

    unsafe {
        std::env::set_var("RYX_DATABASE_URL", "sqlite:///env_default.db");
        std::env::set_var("RYX_DB_LOGS_URL", "sqlite:///env_logs.db");
        std::env::set_var("RYX_POOL_MAX_CONNECTIONS", "20");
    }

    let config = ryx_rs::RyxConfig::load_from_dir(dir);

    // File values take precedence over env vars (matching Python _auto_setup())
    assert_eq!(config.urls.get("default").unwrap(), "sqlite:///file_default.db");
    assert_eq!(config.urls.get("logs").unwrap(), "sqlite:///file_logs.db");
    assert_eq!(config.pool.max_conn, Some(5));

    // Restore
    set_or_remove("RYX_DATABASE_URL", old_default);
    set_or_remove("RYX_DB_LOGS_URL", old_logs);
    set_or_remove("RYX_POOL_MAX_CONNECTIONS", old_max);

    let _ = std::fs::remove_dir_all(&tmp);
}

fn set_or_remove(key: &str, val: Option<String>) {
    match val {
        Some(v) => unsafe { std::env::set_var(key, v) },
        None => unsafe { std::env::remove_var(key) },
    }
}

/// Test raw RyxConfig → pool initialization (SQLite in-memory).
#[tokio::test]
async fn test_config_init_pool() {
    let mut urls = HashMap::new();
    urls.insert("default".into(), "sqlite::memory:".into());
    let config = ryx_rs::RyxConfig {
        urls,
        pool: ryx_rs::config::PoolConfigSection {
            max_conn: Some(1),
            min_conn: Some(1),
            ..Default::default()
        },
        migrations: ryx_rs::config::MigrationsConfig::default(),
    };

    config.init_pool().await.expect("init pool from config");

    let backend = ryx_backend::pool::get(None).unwrap();
    let rows = backend
        .fetch_raw("SELECT 1 AS ok".into(), None)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
}

/// Test loading config from empty directory yields no URLs.
#[test]
fn test_config_empty_dir() {
    let _lock = ENV_LOCK.lock().unwrap();
    let ryx_vars: Vec<(&str, Option<String>)> = ["RYX_DATABASE_URL", "RYX_DB_LOGS_URL", "RYX_DB_REPLICA_URL"]
        .iter()
        .map(|k| (*k, std::env::var(k).ok()))
        .collect();
    for (k, _) in &ryx_vars {
        unsafe { std::env::remove_var(k) };
    }

    let tmp = std::env::temp_dir().join("ryx_test_empty_dir");
    let _ = std::fs::create_dir_all(&tmp);
    let dir = tmp.to_str().expect("utf-8").to_string();

    let config = ryx_rs::RyxConfig::load_from_dir(&dir);
    assert!(
        config.urls.is_empty(),
        "no URLs in empty temp dir; got: {:?}",
        config.urls
    );

    let _ = std::fs::remove_dir_all(&dir);

    for (k, v) in &ryx_vars {
        if let Some(val) = v {
            unsafe { std::env::set_var(k, val) };
        }
    }
}
