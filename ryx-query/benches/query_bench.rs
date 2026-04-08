use criterion::{black_box, criterion_group, criterion_main, Criterion};
use ryx_query::ast::{QNode, SqlValue};
use ryx_query::compiler::compile_q;
use ryx_query::lookups::init_registry;
use ryx_query::Backend;

fn criterion_benchmark(c: &mut Criterion) {
    // Note: Criterion uses a different API for grouping.
    // The above functions were conceptual. Let's use the real Criterion API.

    init_registry();

    let simple_q = QNode::Leaf {
        field: "name".to_string(),
        lookup: "exact".to_string(),
        value: SqlValue::Text("test".to_string()),
        negated: false,
    };
    c.bench_function("compile_q_simple", |b| {
        b.iter(|| {
            let mut values = Vec::new();
            compile_q(
                black_box(&simple_q),
                &mut values,
                black_box(Backend::PostgreSQL),
            )
        })
    });

    let date_q = QNode::Leaf {
        field: "created_at".to_string(),
        lookup: "year__gte".to_string(),
        value: SqlValue::Int(2024),
        negated: false,
    };
    c.bench_function("compile_q_date_transform", |b| {
        b.iter(|| {
            let mut values = Vec::new();
            compile_q(
                black_box(&date_q),
                &mut values,
                black_box(Backend::PostgreSQL),
            )
        })
    });

    let json_q = QNode::Leaf {
        field: "data".to_string(),
        lookup: "has_all".to_string(),
        value: SqlValue::List(vec![
            SqlValue::Text("key1".to_string()),
            SqlValue::Text("key2".to_string()),
            SqlValue::Text("key3".to_string()),
        ]),
        negated: false,
    };
    c.bench_function("compile_q_json_has_all", |b| {
        b.iter(|| {
            let mut values = Vec::new();
            compile_q(
                black_box(&json_q),
                &mut values,
                black_box(Backend::PostgreSQL),
            )
        })
    });

    let complex_q = QNode::Or(vec![
        QNode::And(vec![
            QNode::Leaf {
                field: "active".to_string(),
                lookup: "exact".to_string(),
                value: SqlValue::Bool(true),
                negated: false,
            },
            QNode::Leaf {
                field: "views".to_string(),
                lookup: "gte".to_string(),
                value: SqlValue::Int(100),
                negated: false,
            },
        ]),
        QNode::Leaf {
            field: "featured".to_string(),
            lookup: "exact".to_string(),
            value: SqlValue::Bool(true),
            negated: false,
        },
    ]);
    c.bench_function("compile_q_complex_tree", |b| {
        b.iter(|| {
            let mut values = Vec::new();
            compile_q(
                black_box(&complex_q),
                &mut values,
                black_box(Backend::PostgreSQL),
            )
        })
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
