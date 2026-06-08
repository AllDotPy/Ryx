use std::collections::HashMap;

use ryx_rs::migration::MigrationRunner;
use ryx_rs::model;
use ryx_rs::PoolConfig;

// ── Model definitions ──────────────────────────────────────────

#[model]
#[table("posts")]
struct Post {
    #[field(pk)]
    id: i64,
    title: String,
    body: String,
    published: bool,
}

#[model]
#[table("posts")]
struct PostV2 {
    #[field(pk)]
    id: i64,
    title: String,
    body: String,
    published: bool,
    rating: f64,
}

#[model]
#[table("authors")]
struct Author {
    #[field(pk)]
    id: i64,
    name: String,
}

// ── Sequential test ────────────────────────────────────────────

async fn init_pool() {
    // Remove stale database so each run starts clean
    let _ = std::fs::remove_file("test_rs.db");
    let _ = std::fs::remove_file("test_rs.db-wal");
    let _ = std::fs::remove_file("test_rs.db-shm");

    let mut urls = HashMap::new();
    urls.insert("default".into(), "sqlite:test_rs.db?mode=rwc".into());
    ryx_backend::pool::initialize(urls, PoolConfig {
        max_connections: 1,
        min_connections: 1,
        ..Default::default()
    })
    .await
    .expect("Failed to init SQLite pool");
}

async fn table_exists(name: &str) -> bool {
    let backend = ryx_backend::pool::get(None).unwrap();
    let rows = backend
        .fetch_raw(
            format!("SELECT name FROM sqlite_master WHERE type = 'table' AND name = '{name}'"),
            None,
        )
        .await
        .unwrap();
    !rows.is_empty()
}

async fn pragma_columns(table: &str) -> Vec<String> {
    let backend = ryx_backend::pool::get(None).unwrap();
    let rows = backend
        .fetch_raw(format!("PRAGMA table_info(\"{table}\")"), None)
        .await
        .unwrap();
    rows.iter()
        .filter_map(|r| {
            r.get("name").and_then(|v| match v {
                ryx_rs::SqlValue::Text(s) => Some(s.clone()),
                _ => None,
            })
        })
        .collect()
}

#[tokio::test]
async fn sequential_migration_tests() {
    init_pool().await;

    // Verify basic SQL execution works
    {
        let backend = ryx_backend::pool::get(None).unwrap();
        backend
            .fetch_raw("CREATE TABLE IF NOT EXISTS test_foo (id INTEGER PRIMARY KEY)".into(), None)
            .await
            .expect("Direct table creation should work");
        let rows = backend
            .fetch_raw("SELECT name FROM sqlite_master WHERE type='table' AND name='test_foo'".into(), None)
            .await
            .unwrap();
        assert_eq!(rows.len(), 1, "test_foo should exist after direct CREATE");
    }

    // Test 1: create table
    MigrationRunner::new()
        .live(true)
        .live(true)
        .model::<Post>()
        .run()
        .await
        .expect("Initial migration");
    assert!(table_exists("posts").await);
    assert_eq!(
        pragma_columns("posts").await,
        vec!["id", "title", "body", "published"]
    );

    // Test 2: add column
    MigrationRunner::new()
        .live(true)
        .model::<PostV2>()
        .run()
        .await
        .expect("Add-column migration");
    assert_eq!(
        pragma_columns("posts").await,
        vec!["id", "title", "body", "published", "rating"]
    );

    // Test 3: multiple models
    MigrationRunner::new()
        .live(true)
        .model::<Author>()
        .run()
        .await
        .expect("Add Author table");
    assert!(table_exists("authors").await);

    // Test 4: idempotent
    MigrationRunner::new()
        .live(true)
        .model::<PostV2>()
        .model::<Author>()
        .run()
        .await
        .expect("Idempotent migration");

    // Test 5: plan preview — schema is current, so plan is empty
    let ddl = MigrationRunner::new()
        .model::<PostV2>()
        .model::<Author>()
        .plan()
        .await
        .expect("Plan");
    assert!(ddl.is_empty(), "Plan should be empty when schema is current");
}
