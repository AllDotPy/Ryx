use std::collections::HashMap;

use ryx_rs::migration::MigrationRunner;
use ryx_rs::model;
use ryx_rs::PoolConfig;

// ── Model definitions ─────────────────────────────────

#[model]
#[table("ms_posts")]
struct MsPost {
    #[field(pk)]
    id: i64,
    title: String,
}

#[model]
#[table("ms_authors")]
struct MsAuthor {
    #[field(pk)]
    id: i64,
    name: String,
}

// ── Helpers ────────────────────────────────────────────

fn pg_url() -> Option<String> {
    std::env::var("PG_TEST_URL").ok().or_else(|| {
        Some("postgres://einswilli@localhost/ryx_integration_test".into())
    })
}

async fn init_pg_pool(url: &str) {
    let mut urls = HashMap::new();
    urls.insert("default".into(), url.into());
    ryx_backend::pool::initialize(urls, PoolConfig {
        max_connections: 2,
        min_connections: 1,
        ..Default::default()
    })
    .await
    .expect("Failed to init PG pool");
}

async fn table_exists_in_schema(table: &str, schema: &str) -> bool {
    let backend = ryx_backend::pool::get(None).unwrap();
    let rows = backend
        .fetch_raw(
            format!(
                "SELECT table_name FROM information_schema.tables \
                 WHERE table_schema = '{schema}' AND table_name = '{table}'"
            ),
            None,
        )
        .await
        .unwrap();
    !rows.is_empty()
}

async fn cleanup_schema(schema: &str) {
    let backend = ryx_backend::pool::get(None).unwrap();
    // Drop all tables in the schema first
    let _ = backend
        .fetch_raw(
            format!(
                "SELECT string_agg(format('DROP TABLE IF EXISTS %I.%I CASCADE', \
                 table_schema, table_name), '; ') AS sql \
                 FROM information_schema.tables \
                 WHERE table_schema = '{schema}' AND table_type = 'BASE TABLE'"
            ),
            None,
        )
        .await;

    let _ = backend
        .fetch_raw(format!("DROP SCHEMA IF EXISTS \"{schema}\" CASCADE"), None)
        .await;
}

// ── Multi-schema integration tests ─────────────────────

#[tokio::test]
async fn multi_schema_migration_pipeline() {
    let url = match pg_url() {
        Some(u) => u,
        None => {
            eprintln!("Skipping: PG_TEST_URL not set and default PG may not be available");
            return;
        }
    };

    init_pg_pool(&url).await;

    // Clean up from previous runs (commented out for DB inspection)
    // cleanup_schema("tenant1").await;
    // cleanup_schema("tenant2").await;

    // 1. Run migration in tenant1 schema
    MigrationRunner::new()
        .live(true)
        .schema("tenant1")
        .model::<MsPost>()
        .model::<MsAuthor>()
        .run()
        .await
        .expect("tenant1 migration should succeed");

    assert!(
        table_exists_in_schema("ms_posts", "tenant1").await,
        "ms_posts should exist in tenant1 schema"
    );
    assert!(
        table_exists_in_schema("ms_authors", "tenant1").await,
        "ms_authors should exist in tenant1 schema"
    );

    // 2. Tables should NOT exist in public schema
    assert!(
        !table_exists_in_schema("ms_posts", "public").await,
        "ms_posts should NOT exist in public schema"
    );

    // 3. Run migration in tenant2 schema (same models, different schema)
    MigrationRunner::new()
        .live(true)
        .schema("tenant2")
        .model::<MsPost>()
        .model::<MsAuthor>()
        .run()
        .await
        .expect("tenant2 migration should succeed");

    assert!(
        table_exists_in_schema("ms_posts", "tenant2").await,
        "ms_posts should exist in tenant2 schema"
    );
    assert!(
        table_exists_in_schema("ms_authors", "tenant2").await,
        "ms_authors should exist in tenant2 schema"
    );

    // 4. Tenant1 still has its tables
    assert!(
        table_exists_in_schema("ms_posts", "tenant1").await,
        "tenant1.ms_posts should still exist"
    );

    // 5. Insert data into tenant1 via raw query
    {
        let backend = ryx_backend::pool::get(None).unwrap();
        backend
            .fetch_raw(
                "INSERT INTO \"tenant1\".\"ms_posts\" (title) VALUES ('Post 1'), ('Post 2')"
                    .into(),
                None,
            )
            .await
            .expect("Insert into tenant1 should work");
    }

    // 6. Insert data into tenant2 (independent)
    {
        let backend = ryx_backend::pool::get(None).unwrap();
        backend
            .fetch_raw(
                "INSERT INTO \"tenant2\".\"ms_posts\" (title) VALUES ('Tenant 2 Post')".into(),
                None,
            )
            .await
            .expect("Insert into tenant2 should work");
    }

    // 7. Verify count in tenant1
    {
        let backend = ryx_backend::pool::get(None).unwrap();
        let rows = backend
            .fetch_raw(
                "SELECT count(*) AS cnt FROM \"tenant1\".\"ms_posts\"".into(),
                None,
            )
            .await
            .unwrap();
        let count: i64 = rows[0]
            .get("cnt")
            .and_then(|v| match v {
                ryx_rs::SqlValue::Int(n) => Some(*n),
                _ => None,
            })
            .unwrap_or(0);
        assert_eq!(count, 2, "tenant1 should have 2 posts");
    }

    // 8. Verify count in tenant2
    {
        let backend = ryx_backend::pool::get(None).unwrap();
        let rows = backend
            .fetch_raw(
                "SELECT count(*) AS cnt FROM \"tenant2\".\"ms_posts\"".into(),
                None,
            )
            .await
            .unwrap();
        let count: i64 = rows[0]
            .get("cnt")
            .and_then(|v| match v {
                ryx_rs::SqlValue::Int(n) => Some(*n),
                _ => None,
            })
            .unwrap_or(0);
        assert_eq!(count, 1, "tenant2 should have 1 post");
    }

    // Cleanup (skipped for DB inspection)
    // cleanup_schema("tenant1").await;
    // cleanup_schema("tenant2").await;
}
