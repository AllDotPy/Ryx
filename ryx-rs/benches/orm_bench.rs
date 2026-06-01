use std::collections::HashMap;
use std::sync::OnceLock;

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use ryx_common::PoolConfig;
use ryx_query::ast::SqlValue;
use ryx_query::Backend;
use tokio::runtime::Runtime;

static RT: OnceLock<Runtime> = OnceLock::new();
static BACKEND: OnceLock<std::sync::Arc<dyn ryx_backend::backends::RyxBackend>> = OnceLock::new();

fn rt() -> &'static Runtime {
    RT.get_or_init(|| Runtime::new().expect("tokio runtime"))
}

fn backend() -> &'static std::sync::Arc<dyn ryx_backend::backends::RyxBackend> {
    BACKEND.get_or_init(|| {
        ryx_query::lookups::init_registry();
        let rt = RT.get_or_init(|| Runtime::new().expect("tokio runtime"));
        rt.block_on(init_database())
    })
}

async fn init_database() -> std::sync::Arc<dyn ryx_backend::backends::RyxBackend> {
    let tmp = std::env::temp_dir().join("ryx_bench_orm.db");
    let _ = std::fs::remove_file(&tmp);

    let path = tmp.to_str().expect("utf-8").to_string();
    let url = format!("sqlite:{path}?mode=rwc");

    let mut urls = HashMap::new();
    urls.insert("default".into(), url);

    ryx_backend::pool::initialize(
        urls,
        PoolConfig {
            max_connections: 4,
            min_connections: 1,
            ..Default::default()
        },
    )
    .await
    .expect("pool init");

    let be = ryx_backend::pool::get(None).expect("get backend");

    be.execute_raw(
        "CREATE TABLE posts (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            title TEXT NOT NULL,
            content TEXT NOT NULL,
            author TEXT NOT NULL,
            views INTEGER NOT NULL DEFAULT 0,
            published INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL
        )"
        .into(),
        None,
    )
    .await
    .expect("create table");

    // Insert 5_000 posts
    for chunk in 0..50 {
        let mut values = Vec::new();
        for i in 0..100 {
            let idx = chunk * 100 + i;
            let published = if idx % 3 == 0 { 1 } else { 0 };
            values.push(format!(
                "('Post {idx}', 'Content for post {idx}', 'author_{}', {idx}, {published}, '2024-01-01')",
                idx % 10
            ));
        }
        let batch = values.join(", ");
        be.execute_raw(
            format!("INSERT INTO posts (title, content, author, views, published, created_at) VALUES {batch}"),
            None,
        )
        .await
        .expect("insert batch");
    }

    be
}

// ── QuerySet .all() ───────────────────────────────────────

fn bench_queryset_all(c: &mut Criterion) {
    let be = backend().clone();

    c.bench_function("queryset_all_100", |b| {
        b.to_async(rt()).iter(|| {
            let be = be.clone();
            async move {
                let node = ryx_query::ast::QueryNode::select("posts")
                    .with_backend(Backend::SQLite)
                    .with_limit(100);
                let rows = be.fetch_all_compiled(black_box(node)).await.unwrap();
                black_box(rows.len())
            }
        })
    });
}

fn bench_queryset_all_1000(c: &mut Criterion) {
    let be = backend().clone();

    c.bench_function("queryset_all_1000", |b| {
        b.to_async(rt()).iter(|| {
            let be = be.clone();
            async move {
                let node = ryx_query::ast::QueryNode::select("posts")
                    .with_backend(Backend::SQLite)
                    .with_limit(1000);
                let rows = be.fetch_all_compiled(black_box(node)).await.unwrap();
                black_box(rows.len())
            }
        })
    });
}

// ── QuerySet .filter() ────────────────────────────────────

fn bench_queryset_filter_exact(c: &mut Criterion) {
    let be = backend().clone();
    let node = ryx_query::ast::QueryNode::select("posts")
        .with_backend(Backend::SQLite)
        .with_filter(ryx_query::ast::FilterNode {
            field: "author".into(),
            lookup: "exact".to_string(),
            value: SqlValue::Text("author_5".into()),
            negated: false,
        })
        .with_limit(50);

    c.bench_function("queryset_filter_exact", |b| {
        b.to_async(rt()).iter(|| {
            let be = be.clone();
            let n = node.clone();
            async move {
                let rows = be.fetch_all_compiled(black_box(n)).await.unwrap();
                black_box(rows.len())
            }
        })
    });
}

fn bench_queryset_filter_gte(c: &mut Criterion) {
    let be = backend().clone();
    let node = ryx_query::ast::QueryNode::select("posts")
        .with_backend(Backend::SQLite)
        .with_q(ryx_query::ast::QNode::Leaf {
            field: "views".into(),
            lookup: "gte".to_string(),
            value: SqlValue::Int(2500),
            negated: false,
        })
        .with_limit(50);

    c.bench_function("queryset_filter_gte", |b| {
        b.to_async(rt()).iter(|| {
            let be = be.clone();
            let n = node.clone();
            async move {
                let rows = be.fetch_all_compiled(black_box(n)).await.unwrap();
                black_box(rows.len())
            }
        })
    });
}

fn bench_queryset_filter_complex(c: &mut Criterion) {
    let be = backend().clone();
    let node = ryx_query::ast::QueryNode::select("posts")
        .with_backend(Backend::SQLite)
        .with_q(ryx_query::ast::QNode::Or(vec![
            ryx_query::ast::QNode::And(vec![
                ryx_query::ast::QNode::Leaf {
                    field: "published".into(),
                    lookup: "exact".to_string(),
                    value: SqlValue::Int(1),
                    negated: false,
                },
                ryx_query::ast::QNode::Leaf {
                    field: "views".into(),
                    lookup: "gte".to_string(),
                    value: SqlValue::Int(1000),
                    negated: false,
                },
            ]),
            ryx_query::ast::QNode::Leaf {
                field: "author".into(),
                lookup: "exact".to_string(),
                value: SqlValue::Text("author_0".into()),
                negated: false,
            },
        ]))
        .with_order_by(ryx_query::ast::OrderByClause {
            field: "views".into(),
            direction: ryx_query::ast::SortDirection::Desc,
        })
        .with_limit(20);

    c.bench_function("queryset_filter_complex", |b| {
        b.to_async(rt()).iter(|| {
            let be = be.clone();
            let n = node.clone();
            async move {
                let rows = be.fetch_all_compiled(black_box(n)).await.unwrap();
                black_box(rows.len())
            }
        })
    });
}

// ── QuerySet .count() ─────────────────────────────────────

fn bench_queryset_count(c: &mut Criterion) {
    let be = backend().clone();
    let node = ryx_query::ast::QueryNode::count("posts").with_backend(Backend::SQLite);

    c.bench_function("queryset_count", |b| {
        b.to_async(rt()).iter(|| {
            let be = be.clone();
            let n = node.clone();
            async move {
                let cnt = be.fetch_count_compiled(black_box(n)).await.unwrap();
                black_box(cnt)
            }
        })
    });
}

fn bench_queryset_count_filtered(c: &mut Criterion) {
    let be = backend().clone();
    let mut node = ryx_query::ast::QueryNode::count("posts").with_backend(Backend::SQLite);

    c.bench_function("queryset_count_filtered", |b| {
        b.to_async(rt()).iter(|| {
            let be = be.clone();
            node.operation = ryx_query::ast::QueryOperation::Count;
            let n = node.clone();
            async move {
                let cnt = be.fetch_count_compiled(black_box(n)).await.unwrap();
                black_box(cnt)
            }
        })
    });
}

// ── QuerySet .filter().order_by() ─────────────────────────

fn bench_queryset_order_by(c: &mut Criterion) {
    let be = backend().clone();
    let node = ryx_query::ast::QueryNode::select("posts")
        .with_backend(Backend::SQLite)
        .with_filter(ryx_query::ast::FilterNode {
            field: "published".into(),
            lookup: "exact".to_string(),
            value: SqlValue::Int(1),
            negated: false,
        })
        .with_order_by(ryx_query::ast::OrderByClause {
            field: "created_at".into(),
            direction: ryx_query::ast::SortDirection::Desc,
        })
        .with_order_by(ryx_query::ast::OrderByClause {
            field: "views".into(),
            direction: ryx_query::ast::SortDirection::Desc,
        })
        .with_limit(30);

    c.bench_function("queryset_filter_order_by", |b| {
        b.to_async(rt()).iter(|| {
            let be = be.clone();
            let n = node.clone();
            async move {
                let rows = be.fetch_all_compiled(black_box(n)).await.unwrap();
                black_box(rows.len())
            }
        })
    });
}

// ── QuerySet .insert() / create() ────────────────────────

fn bench_queryset_create(c: &mut Criterion) {
    let be = backend().clone();
    let mut base = ryx_query::ast::QueryNode::select("posts").with_backend(Backend::SQLite);

    c.bench_function("queryset_create", |b| {
        b.to_async(rt()).iter(|| {
            let be = be.clone();
            base.operation = ryx_query::ast::QueryOperation::Insert {
                values: vec![
                    ("title".into(), SqlValue::Text("Bench post".into())),
                    ("content".into(), SqlValue::Text("Bench content".into())),
                    ("author".into(), SqlValue::Text("benchmark".into())),
                    ("views".into(), SqlValue::Int(0)),
                    ("published".into(), SqlValue::Int(1)),
                    ("created_at".into(), SqlValue::Text("2024-06-01".into())),
                ],
                returning_id: true,
            };
            let n = base.clone();
            async move {
                let result = be.execute_compiled(black_box(n)).await.unwrap();
                black_box(result)
            }
        })
    });
}

// ── QuerySet .update() ────────────────────────────────────

fn bench_queryset_update(c: &mut Criterion) {
    let be = backend().clone();
    let mut base = ryx_query::ast::QueryNode::select("posts").with_backend(Backend::SQLite);

    c.bench_function("queryset_update_all", |b| {
        b.to_async(rt()).iter(|| {
            let be = be.clone();
            base.operation = ryx_query::ast::QueryOperation::Update {
                assignments: vec![("views".into(), SqlValue::Int(9999))],
            };
            let n = base.clone();
            async move {
                let result = be.execute_compiled(black_box(n)).await.unwrap();
                black_box(result)
            }
        })
    });
}

// ── QuerySet .delete() ────────────────────────────────────

fn bench_queryset_delete(c: &mut Criterion) {
    let be = backend().clone();
    let node = ryx_query::ast::QueryNode::delete("posts").with_backend(Backend::SQLite);

    // Reinsert rows before benchmark so there's something to delete
    rt().block_on(async {
        be.execute_raw(
            "INSERT INTO posts (title, content, author, views, published, created_at) \
             VALUES ('del', 'del', 'del', 0, 0, '2024-01-01')"
                .into(),
            None,
        )
        .await
        .unwrap();
    });

    c.bench_function("queryset_delete", |b| {
        b.to_async(rt()).iter(|| {
            let be = be.clone();
            let n = node.clone();
            async move {
                let result = be.execute_compiled(black_box(n)).await.unwrap();
                black_box(result)
            }
        })
    });
}

// ── QueryNode compilation (no DB query) ───────────────────

fn bench_compile_select_simple(c: &mut Criterion) {
    let node = ryx_query::ast::QueryNode::select("posts").with_backend(Backend::SQLite);

    c.bench_function("compile_select_simple", |b| {
        b.iter(|| {
            let result = ryx_query::compiler::compile(black_box(&node)).unwrap();
            black_box(result)
        })
    });
}

fn bench_compile_select_complex(c: &mut Criterion) {
    let node = ryx_query::ast::QueryNode::select("posts")
        .with_backend(Backend::SQLite)
        .with_q(ryx_query::ast::QNode::And(vec![
            ryx_query::ast::QNode::Leaf {
                field: "views".into(),
                lookup: "gte".to_string(),
                value: SqlValue::Int(100),
                negated: false,
            },
            ryx_query::ast::QNode::Leaf {
                field: "published".into(),
                lookup: "exact".to_string(),
                value: SqlValue::Int(1),
                negated: false,
            },
        ]))
        .with_order_by(ryx_query::ast::OrderByClause {
            field: "views".into(),
            direction: ryx_query::ast::SortDirection::Desc,
        })
        .with_limit(50);

    c.bench_function("compile_select_complex", |b| {
        b.iter(|| {
            let result = ryx_query::compiler::compile(black_box(&node)).unwrap();
            black_box(result)
        })
    });
}

// ── Model / row conversion ────────────────────────────────

fn bench_row_to_hashmap(c: &mut Criterion) {
    use ryx_backend::backends::{RowMapping, RowView};

    let row = RowView {
        values: vec![
            SqlValue::Int(42),
            SqlValue::Text("Hello World".into()),
            SqlValue::Text("Content here".into()),
            SqlValue::Text("author_3".into()),
            SqlValue::Int(1500),
            SqlValue::Int(1),
            SqlValue::Text("2024-01-15".into()),
        ],
        mapping: std::sync::Arc::new(RowMapping {
            columns: vec![
                "id".into(),
                "title".into(),
                "content".into(),
                "author".into(),
                "views".into(),
                "published".into(),
                "created_at".into(),
            ],
        }),
    };

    c.bench_function("row_to_hashmap_7_fields", |b| {
        b.iter(|| {
            let mut map = std::collections::HashMap::new();
            for col in ["id", "title", "content", "author", "views", "published", "created_at"] {
                if let Some(val) = row.get(col) {
                    map.insert(col.to_string(), val.clone());
                }
            }
            black_box(map)
        })
    });
}

criterion_group!(
    benches,
    bench_queryset_all,
    bench_queryset_all_1000,
    bench_queryset_filter_exact,
    bench_queryset_filter_gte,
    bench_queryset_filter_complex,
    bench_queryset_count,
    bench_queryset_count_filtered,
    bench_queryset_order_by,
    bench_queryset_create,
    bench_queryset_update,
    bench_queryset_delete,
    bench_compile_select_simple,
    bench_compile_select_complex,
    bench_row_to_hashmap,
);
criterion_main!(benches);
