use std::collections::HashMap;
use std::sync::OnceLock;

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use ryx_backend::backends::RyxBackend;
use ryx_backend::pool;
use ryx_common::PoolConfig;
use ryx_query::ast::{
    FilterNode, OrderByClause, QNode, QueryNode, QueryOperation, SortDirection, SqlValue,
};
use ryx_query::Backend;
use tokio::runtime::Runtime;

static RT: OnceLock<Runtime> = OnceLock::new();
static BACKEND: OnceLock<ArcBackend> = OnceLock::new();

type ArcBackend = std::sync::Arc<dyn RyxBackend>;

fn rt() -> &'static Runtime {
    RT.get_or_init(|| Runtime::new().expect("tokio runtime"))
}

fn backend() -> &'static ArcBackend {
    BACKEND.get_or_init(|| {
        ryx_query::lookups::init_registry();
        let rt = RT.get_or_init(|| Runtime::new().expect("tokio runtime"));
        rt.block_on(init_database())
    })
}

async fn init_database() -> ArcBackend {
    let tmp = std::env::temp_dir().join("ryx_bench_backend.db");
    let _ = std::fs::remove_file(&tmp);

    let path = tmp.to_str().expect("utf-8").to_string();
    let url = format!("sqlite:{path}?mode=rwc");

    let mut urls = HashMap::new();
    urls.insert("default".into(), url);

    pool::initialize(
        urls,
        PoolConfig {
            max_connections: 4,
            min_connections: 1,
            ..Default::default()
        },
    )
    .await
    .expect("pool init");

    let be = pool::get(None).expect("get backend");

    be.execute_raw(
        "CREATE TABLE bench_items (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            value REAL NOT NULL,
            active INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL
        )"
        .into(),
        None,
    )
    .await
    .expect("create table");

    // Insert 10_000 rows in batches of 100
    for chunk in 0..100 {
        let mut values = Vec::new();
        for i in 0..100 {
            let idx = chunk * 100 + i;
            let active = if idx % 2 == 0 { 1 } else { 0 };
            values.push(format!("('item_{idx}', {idx}.5, {active}, '2024-01-01')"));
        }
        let batch = values.join(", ");
        be.execute_raw(
            format!("INSERT INTO bench_items (name, value, active, created_at) VALUES {batch}"),
            None,
        )
        .await
        .expect("insert batch");
    }

    be
}

// ── Raw SQL ─────────────────────────────────────────────────

fn bench_raw_select_single(c: &mut Criterion) {
    let be = backend().clone();
    c.bench_function("raw_select_single", |b| {
        b.to_async(rt()).iter(|| {
            let be = be.clone();
            async move {
                let rows = be
                    .fetch_raw(black_box("SELECT * FROM bench_items WHERE id = 5000".into()), None)
                    .await
                    .unwrap();
                black_box(rows)
            }
        })
    });
}

fn bench_raw_select_100(c: &mut Criterion) {
    let be = backend().clone();
    c.bench_function("raw_select_100", |b| {
        b.to_async(rt()).iter(|| {
            let be = be.clone();
            async move {
                let rows = be
                    .fetch_raw(black_box("SELECT * FROM bench_items LIMIT 100".into()), None)
                    .await
                    .unwrap();
                black_box(rows)
            }
        })
    });
}

fn bench_raw_select_count(c: &mut Criterion) {
    let be = backend().clone();
    c.bench_function("raw_select_count", |b| {
        b.to_async(rt()).iter(|| {
            let be = be.clone();
            async move {
                let rows = be
                    .fetch_raw(
                        black_box("SELECT COUNT(*) as cnt FROM bench_items WHERE active = 1".into()),
                        None,
                    )
                    .await
                    .unwrap();
                black_box(rows)
            }
        })
    });
}

fn bench_raw_insert_single(c: &mut Criterion) {
    let be = backend().clone();
    c.bench_function("raw_insert_single", |b| {
        b.to_async(rt()).iter(|| {
            let be = be.clone();
            async move {
                be.execute_raw(
                    black_box(
                        "INSERT INTO bench_items (name, value, active, created_at) \
                         VALUES ('new_item', 999.0, 1, '2024-06-01')"
                            .into(),
                    ),
                    None,
                )
                .await
                .unwrap();
            }
        })
    });
}

fn bench_raw_update(c: &mut Criterion) {
    let be = backend().clone();
    c.bench_function("raw_update_all", |b| {
        b.to_async(rt()).iter(|| {
            let be = be.clone();
            async move {
                be.execute_raw(
                    black_box("UPDATE bench_items SET value = value + 0.5".into()),
                    None,
                )
                .await
                .unwrap();
            }
        })
    });
}

fn bench_raw_delete_single(c: &mut Criterion) {
    let be = backend().clone();
    c.bench_function("raw_delete_single", |b| {
        b.to_async(rt()).iter(|| {
            let be = be.clone();
            async move {
                be.execute_raw(
                    black_box("DELETE FROM bench_items WHERE id = 9999".into()),
                    None,
                )
                .await
                .unwrap();
            }
        })
    });
}

// ── Compiled query ──────────────────────────────────────────

fn bench_compiled_select_100(c: &mut Criterion) {
    let be = backend().clone();
    let mut node = QueryNode::select("bench_items")
        .with_backend(Backend::SQLite)
        .with_limit(100);

    c.bench_function("compiled_select_100", |b| {
        b.to_async(rt()).iter(|| {
            let be = be.clone();
            node.limit = Some(100);
            let n = node.clone();
            async move {
                let rows = be.fetch_all_compiled(black_box(n)).await.unwrap();
                black_box(rows)
            }
        })
    });
}

fn bench_compiled_select_filtered(c: &mut Criterion) {
    let be = backend().clone();
    let node = QueryNode::select("bench_items")
        .with_backend(Backend::SQLite)
        .with_filter(FilterNode {
            field: "active".into(),
            lookup: "exact".to_string(),
            value: SqlValue::Int(1),
            negated: false,
        })
        .with_limit(50);

    c.bench_function("compiled_select_filtered", |b| {
        b.to_async(rt()).iter(|| {
            let be = be.clone();
            let n = node.clone();
            async move {
                let rows = be.fetch_all_compiled(black_box(n)).await.unwrap();
                black_box(rows)
            }
        })
    });
}

fn bench_compiled_select_complex(c: &mut Criterion) {
    let be = backend().clone();
    let node = QueryNode::select("bench_items")
        .with_backend(Backend::SQLite)
        .with_q(QNode::And(vec![
            QNode::Leaf {
                field: "value".into(),
                lookup: "gte".to_string(),
                value: SqlValue::Float(100.0),
                negated: false,
            },
            QNode::Leaf {
                field: "value".into(),
                lookup: "lte".to_string(),
                value: SqlValue::Float(5000.0),
                negated: false,
            },
            QNode::Leaf {
                field: "active".into(),
                lookup: "exact".to_string(),
                value: SqlValue::Int(1),
                negated: false,
            },
        ]))
        .with_order_by(OrderByClause {
            field: "name".into(),
            direction: SortDirection::Desc,
        })
        .with_limit(50);

    c.bench_function("compiled_select_complex", |b| {
        b.to_async(rt()).iter(|| {
            let be = be.clone();
            let n = node.clone();
            async move {
                let rows = be.fetch_all_compiled(black_box(n)).await.unwrap();
                black_box(rows)
            }
        })
    });
}

fn bench_compiled_count(c: &mut Criterion) {
    let be = backend().clone();
    let node = QueryNode::count("bench_items").with_backend(Backend::SQLite);

    c.bench_function("compiled_count", |b| {
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

fn bench_compiled_insert(c: &mut Criterion) {
    let be = backend().clone();
    let mut node = QueryNode::select("bench_items").with_backend(Backend::SQLite);

    c.bench_function("compiled_insert", |b| {
        b.to_async(rt()).iter(|| {
            let be = be.clone();
            node.operation = QueryOperation::Insert {
                values: vec![
                    ("name".into(), SqlValue::Text("c_insert".into())),
                    ("value".into(), SqlValue::Float(42.0)),
                    ("active".into(), SqlValue::Int(1)),
                    ("created_at".into(), SqlValue::Text("2024-06-01".into())),
                ],
                returning_id: true,
            };
            let n = node.clone();
            async move {
                let result = be.execute_compiled(black_box(n)).await.unwrap();
                black_box(result)
            }
        })
    });
}

fn bench_compiled_update(c: &mut Criterion) {
    let be = backend().clone();
    let mut node = QueryNode::select("bench_items").with_backend(Backend::SQLite);

    c.bench_function("compiled_update", |b| {
        b.to_async(rt()).iter(|| {
            let be = be.clone();
            node.operation = QueryOperation::Update {
                assignments: vec![("value".into(), SqlValue::Float(999.0))],
            };
            let n = node.clone();
            async move {
                let result = be.execute_compiled(black_box(n)).await.unwrap();
                black_box(result)
            }
        })
    });
}

// ── Row decode (no DB query) ────────────────────────────────

fn bench_decode_row_small(c: &mut Criterion) {
    use ryx_query::ast::SqlValue;

    let row = ryx_backend::backends::RowView {
        values: vec![
            SqlValue::Int(1),
            SqlValue::Text("hello".into()),
            SqlValue::Float(3.14),
            SqlValue::Bool(true),
        ],
        mapping: std::sync::Arc::new(ryx_backend::backends::RowMapping {
            columns: vec!["id".into(), "name".into(), "value".into(), "active".into()],
        }),
    };

    c.bench_function("decode_row_4_fields", |b| {
        b.iter(|| {
            let id = black_box(row.get("id"));
            let name = black_box(row.get("name"));
            let val = black_box(row.get("value"));
            let act = black_box(row.get("active"));
            black_box((id, name, val, act))
        })
    });
}

criterion_group!(
    benches,
    bench_raw_select_single,
    bench_raw_select_100,
    bench_raw_select_count,
    bench_raw_insert_single,
    bench_raw_update,
    bench_raw_delete_single,
    bench_compiled_select_100,
    bench_compiled_select_filtered,
    bench_compiled_select_complex,
    bench_compiled_count,
    bench_compiled_insert,
    bench_compiled_update,
    bench_decode_row_small,
);
criterion_main!(benches);
