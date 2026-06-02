use std::collections::HashMap;

use ryx_rs::migration::MigrationRunner;
use ryx_rs::model;
use ryx_rs::PoolConfig;
use ryx_rs::Q;

// ── Model definitions ─────────────────────────────────────

#[model]
#[table("pipeline_posts")]
struct Post {
    #[field(pk)]
    id: i64,
    title: String,
    content: String,
    author: String,
    views: i64,
    published: bool,
}

#[model]
#[table("pipeline_authors")]
struct Author {
    #[field(pk)]
    id: i64,
    name: String,
    email: String,
    age: i64,
}

// ── Test helpers ──────────────────────────────────────────

async fn init_pool(db_name: &str) {
    ryx_query::lookups::init_registry();

    let _ = std::fs::remove_file(db_name);
    let _ = std::fs::remove_file(&format!("{db_name}-wal"));
    let _ = std::fs::remove_file(&format!("{db_name}-shm"));

    let mut urls = HashMap::new();
    urls.insert("default".into(), format!("sqlite:{db_name}?mode=rwc"));
    ryx_backend::pool::initialize(urls, PoolConfig {
        max_connections: 1,
        min_connections: 1,
        ..Default::default()
    })
    .await
    .expect("Failed to init SQLite pool");
}

fn backend() -> std::sync::Arc<dyn ryx_backend::backends::RyxBackend> {
    ryx_backend::pool::get(None).expect("get backend")
}

async fn seed_data() {
    let be = backend();
    for i in 0..20 {
        let title = format!("Post {i}");
        let content = format!("Content {i}");
        let author = format!("author_{}", i % 4);
        let views = i * 100;
        let published = if i % 2 == 0 { 1 } else { 0 };
        be.execute_raw(
            format!(
                "INSERT INTO pipeline_posts (title, content, author, views, published) \
                 VALUES ('{title}', '{content}', '{author}', {views}, {published})"
            ),
            None,
        )
        .await
        .expect("seed post");
    }

    let authors = vec![
        ("Alice", "alice@test.com", 30),
        ("Bob", "bob@test.com", 25),
        ("Charlie", "charlie@test.com", 35),
    ];
    for (name, email, age) in &authors {
        be.execute_raw(
            format!(
                "INSERT INTO pipeline_authors (name, email, age) VALUES ('{name}', '{email}', {age})"
            ),
            None,
        )
        .await
        .expect("seed author");
    }
}

// ── Full pipeline test ────────────────────────────────────

#[tokio::test]
async fn full_pipeline_test() {
    let db = "full_pipeline_test.db";
    init_pool(db).await;

    // 1. Create tables via MigrationRunner
    MigrationRunner::new()
        .model::<Post>()
        .model::<Author>()
        .run()
        .await
        .expect("create tables");

    seed_data().await;

    // 2. QuerySet .all()
    let posts = ryx_rs::objects::ObjectsManager::<Post>::new()
        .all()
        .all()
        .await
        .expect("all posts");
    assert_eq!(posts.len(), 20, "should have 20 posts");

    // 3. QuerySet .filter() with exact match
    let filtered = ryx_rs::objects::ObjectsManager::<Post>::new()
        .filter("author", "author_1")
        .all()
        .await
        .expect("filtered posts");
    assert_eq!(filtered.len(), 5, "author_1 should have 5 posts");

    // 4. QuerySet .filter() with chained calls via QuerySet directly
    let chained = {
        use ryx_rs::queryset::QuerySet;
        QuerySet::<Post>::new("pipeline_posts")
            .filter(("published", 1i64))
            .filter(("author", "author_0"))
            .all()
            .await
            .expect("chained filters")
    };
    assert_eq!(chained.len(), 5, "published + author_0 should have 5 posts");

    // 5. QuerySet .count() via QuerySet with Q for gte
    let count = {
        use ryx_rs::queryset::QuerySet;
        QuerySet::<Post>::new("pipeline_posts")
            .filter(Q::new("views__gte", 100i64))
            .count()
            .await
            .expect("count")
    };
    assert!(count > 0, "should have posts with views >= 100");

    // 6. QuerySet .exists()
    let exists = ryx_rs::objects::ObjectsManager::<Post>::new()
        .filter("title", "Post 0")
        .exists()
        .await
        .expect("exists");
    assert!(exists, "Post 0 should exist");

    let not_exists = ryx_rs::objects::ObjectsManager::<Post>::new()
        .filter("title", "Post 999")
        .exists()
        .await
        .expect("exists check");
    assert!(!not_exists, "Post 999 should not exist");

    // 7. QuerySet .get()
    let post = ryx_rs::objects::ObjectsManager::<Post>::new()
        .get("id", 1i64)
        .all()
        .await
        .expect("get post by id");
    assert_eq!(post.len(), 1, "get should return 1 post");
    assert_eq!(post[0].title, "Post 0");

    // 8. QuerySet .first()
    let first = ryx_rs::objects::ObjectsManager::<Post>::new()
        .filter("published", 1i64)
        .first()
        .await
        .expect("first post");
    assert!(first.is_some(), "first published post should exist");

    // 9. QuerySet .order_by() (with .all()) — using raw QuerySet
    {
        use ryx_rs::queryset::QuerySet;
        let ordered = QuerySet::<Post>::new("pipeline_posts")
            .order_by("views")
            .all()
            .await
            .expect("ordered posts");
        assert_eq!(ordered.len(), 20);
        // First item should have the least views
        assert_eq!(ordered[0].views, 0);
    }

    // 10. QuerySet .limit() / .offset()
    {
        use ryx_rs::queryset::QuerySet;
        let limited = QuerySet::<Post>::new("pipeline_posts")
            .limit(5)
            .all()
            .await
            .expect("limited posts");
        assert_eq!(limited.len(), 5, "should return 5 posts");

        let offset = QuerySet::<Post>::new("pipeline_posts")
            .limit(5)
            .offset(15)
            .all()
            .await
            .expect("offset posts");
        assert_eq!(offset.len(), 5, "should return 5 posts with offset");
    }

    // 11. QuerySet .update()
    let updated = ryx_rs::objects::ObjectsManager::<Post>::new()
        .filter("author", "author_3")
        .update(vec![("views", 9999i64)])
        .await
        .expect("update posts");
    assert!(updated > 0, "should update some posts");

    // 12. QuerySet .delete()
    let deleted = ryx_rs::objects::ObjectsManager::<Post>::new()
        .filter("title", "Post 19")
        .delete()
        .await
        .expect("delete post");
    assert_eq!(deleted, 1, "should delete exactly 1 post");

    // Verify deletion
    let remaining = ryx_rs::objects::ObjectsManager::<Post>::new()
        .all()
        .all()
        .await
        .expect("remaining posts");
    assert_eq!(remaining.len(), 19, "19 posts should remain after deletion");

    // 13. QuerySet .values()
    {
        use ryx_rs::queryset::QuerySet;
        let values = QuerySet::<Post>::new("pipeline_posts")
            .filter(("title", "Post 5"))
            .values(&["title", "views"])
            .await
            .expect("values");
        assert_eq!(values.len(), 1);
        match values[0].get("title").unwrap() {
            ryx_rs::SqlValue::Text(t) => assert_eq!(t, "Post 5"),
            _ => panic!("expected Text"),
        }
    }

    // 14. QuerySet .values_list()
    {
        use ryx_rs::queryset::QuerySet;
        let list = QuerySet::<Post>::new("pipeline_posts")
            .filter(("title", "Post 5"))
            .values_list(&["title", "views"])
            .await
            .expect("values_list");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].len(), 2);
    }

    // 15. QuerySet .aggregate()
    {
        use ryx_rs::agg::{sum, count as agg_count};
        use ryx_rs::queryset::QuerySet;
        let aggs = QuerySet::<Post>::new("pipeline_posts")
            .aggregate(&[agg_count("total", "id"), sum("total_views", "views")])
            .await
            .expect("aggregate");
        assert_eq!(aggs.len(), 2, "should return 2 aggregate values");
        // Total posts should be 19 (1 deleted)
        assert!(aggs.contains_key("total"));
        assert!(aggs.contains_key("total_views"));
    }

    // 16. QuerySet .annotate()
    {
        use ryx_rs::agg::count;
        use ryx_rs::queryset::QuerySet;
        let annotated = QuerySet::<Post>::new("pipeline_posts")
            .annotate(&[count("cnt", "id")])
            .await
            .expect("annotate");
        assert!(!annotated.is_empty(), "should return annotated rows");
        for row in &annotated {
            assert!(row.contains_key("cnt"), "each row should have cnt annotation");
        }
    }

    // 17. QuerySet .distinct()
    {
        use ryx_rs::queryset::QuerySet;
        let distinct_authors = QuerySet::<Post>::new("pipeline_posts")
            .distinct()
            .values(&["author"])
            .await
            .expect("distinct authors");
        // We have 4 unique authors (author_0 through author_3)
        assert_eq!(distinct_authors.len(), 4, "4 distinct authors");
    }

    // 18. Multiple models — query authors
    let authors = ryx_rs::objects::ObjectsManager::<Author>::new()
        .all()
        .all()
        .await
        .expect("all authors");
    assert_eq!(authors.len(), 3, "should have 3 authors");

    let bob = ryx_rs::objects::ObjectsManager::<Author>::new()
        .filter("name", "Bob")
        .all()
        .await
        .expect("bob");
    assert_eq!(bob.len(), 1);
    assert_eq!(bob[0].email, "bob@test.com");
}
