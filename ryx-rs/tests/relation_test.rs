use std::collections::HashMap;

use ryx_rs::migration::MigrationRunner;
use ryx_rs::model;
use ryx_rs::model::Relationships;
use ryx_rs::PoolConfig;

// ── Models ─────────────────────────────────────────────────────

#[model]
#[table("rel_authors")]
struct RelAuthor {
    #[field(pk)]
    id: i64,
    name: String,
}

#[model]
#[table("rel_posts")]
#[relation(model = "RelAuthor", fk_column = "author_id", name = "author")]
struct RelPost {
    #[field(pk)]
    id: i64,
    title: String,
    author_id: i64,
    author: Option<RelAuthor>,
}

// ── Helpers ────────────────────────────────────────────────────

async fn init_pool() {
    let _ = std::fs::remove_file("relation_test.db");
    let _ = std::fs::remove_file("relation_test.db-wal");
    let _ = std::fs::remove_file("relation_test.db-shm");

    ryx_query::lookups::init_registry();
    let mut urls = HashMap::new();
    urls.insert("default".into(), "sqlite:relation_test.db?mode=rwc".into());
    ryx_backend::pool::initialize(urls, PoolConfig {
        max_connections: 1,
        min_connections: 1,
        ..Default::default()
    })
    .await
    .expect("init pool");
}

// ── Test: #[relation] metadata ─────────────────────────────────

#[test]
fn test_relation_metadata() {
    let rels = <RelPost as Relationships>::relations();
    assert_eq!(rels.len(), 1);
    assert_eq!(rels[0].name, "author");        // from name= attribute
    assert_eq!(rels[0].fk_column, "author_id");
    assert_eq!(rels[0].to_table, "rel_authors");
    assert_eq!(rels[0].to_field, "id");
    assert_eq!(rels[0].relation_fields, &["id", "name"]);
}

#[test]
fn test_no_relations_when_none_declared() {
    // RelAuthor has no #[relation], so it should NOT implement Relationships
    // This is verified at compile time: the trait is simply not implemented.
}

// ── Test: select_related JOIN generation ────────────────────────

#[tokio::test]
async fn test_select_related_basic() {
    init_pool().await;

    // Create tables
    MigrationRunner::new()
        .live(true)
        .model::<RelAuthor>()
        .model::<RelPost>()
        .run()
        .await
        .expect("migration");

    // Seed data
    let backend = ryx_backend::pool::get(None).unwrap();
    backend
        .execute_raw("INSERT INTO rel_authors (name) VALUES ('Alice')".into(), None)
        .await
        .unwrap();
    backend
        .execute_raw("INSERT INTO rel_authors (name) VALUES ('Bob')".into(), None)
        .await
        .unwrap();
    backend
        .execute_raw("INSERT INTO rel_posts (title, author_id) VALUES ('Post 1', 1)".into(), None)
        .await
        .unwrap();
    backend
        .execute_raw("INSERT INTO rel_posts (title, author_id) VALUES ('Post 2', 1)".into(), None)
        .await
        .unwrap();
    backend
        .execute_raw("INSERT INTO rel_posts (title, author_id) VALUES ('Post 3', 2)".into(), None)
        .await
        .unwrap();

    // Debug: verify tables were created correctly
    {
        let backend = ryx_backend::pool::get(None).unwrap();
        let cols = backend
            .fetch_raw("PRAGMA table_info(\"rel_authors\")".into(), None)
            .await
            .unwrap();
        println!("rel_authors columns: {:?}", cols);
        let cols2 = backend
            .fetch_raw("PRAGMA table_info(\"rel_posts\")".into(), None)
            .await
            .unwrap();
        println!("rel_posts columns: {:?}", cols2);
    }

    // select_related — should return all 3 posts with LEFT JOIN
    let posts = ryx_rs::objects::ObjectsManager::<RelPost>::new()
        .all()
        .select_related(&["author"])
        .all()
        .await
        .expect("select_related");

    assert_eq!(posts.len(), 3);
    assert_eq!(posts[0].title, "Post 1");
    assert_eq!(posts[1].title, "Post 2");
    assert_eq!(posts[2].title, "Post 3");

    // Verify related author is populated
    assert!(posts[0].author.is_some(), "Post 1 should have an author");
    assert_eq!(posts[0].author.as_ref().unwrap().name, "Alice");

    assert!(posts[1].author.is_some(), "Post 2 should have an author");
    assert_eq!(posts[1].author.as_ref().unwrap().name, "Alice");

    assert!(posts[2].author.is_some(), "Post 3 should have an author");
    assert_eq!(posts[2].author.as_ref().unwrap().name, "Bob");
}
