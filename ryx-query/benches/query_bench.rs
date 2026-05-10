use criterion::{Criterion, black_box, criterion_group, criterion_main};
use ryx_query::Backend;
use ryx_query::ast::{QNode, QueryNode, QueryOperation, SqlValue};
use ryx_query::compiler::compilr::SqlWriter;
use ryx_query::compiler::{compile, compile_q};
use ryx_query::lookups::init_registry;

fn criterion_benchmark(c: &mut Criterion) {
    // Note: Criterion uses a different API for grouping.
    // The above functions were conceptual. Let's use the real Criterion API.

    init_registry();

    let simple_q = QNode::Leaf {
        field: "name".into(),
        lookup: "exact".to_string(),
        value: SqlValue::Text("test".to_string()),
        negated: false,
    };
    c.bench_function("compile_q_simple", |b| {
        b.iter(|| {
            let mut values = smallvec::SmallVec::<[SqlValue; 8]>::new();
            let mut w = SqlWriter::new_emit();
            compile_q(
                black_box(&simple_q),
                &mut values,
                black_box(Backend::PostgreSQL),
                &mut w,
            )
        })
    });

    let date_q = QNode::Leaf {
        field: "created_at".into(),
        lookup: "year__gte".to_string(),
        value: SqlValue::Int(2024),
        negated: false,
    };
    c.bench_function("compile_q_date_transform", |b| {
        b.iter(|| {
            let mut values = smallvec::SmallVec::<[SqlValue; 8]>::new();
            let mut w = SqlWriter::new_emit();
            compile_q(
                black_box(&date_q),
                &mut values,
                black_box(Backend::PostgreSQL),
                &mut w,
            )
        })
    });

    let json_q = QNode::Leaf {
        field: "data".into(),
        lookup: "has_all".to_string(),
        value: SqlValue::List(smallvec::smallvec![
            Box::new(SqlValue::Text("key1".to_string())),
            Box::new(SqlValue::Text("key2".to_string())),
            Box::new(SqlValue::Text("key3".to_string())),
        ]),
        negated: false,
    };
    c.bench_function("compile_q_json_has_all", |b| {
        b.iter(|| {
            let mut values = smallvec::SmallVec::<[SqlValue; 8]>::new();
            let mut w = SqlWriter::new_emit();
            compile_q(
                black_box(&json_q),
                &mut values,
                black_box(Backend::PostgreSQL),
                &mut w,
            )
        })
    });

    let complex_q = QNode::Or(vec![
        QNode::And(vec![
            QNode::Leaf {
                field: "active".into(),
                lookup: "exact".to_string(),
                value: SqlValue::Bool(true),
                negated: false,
            },
            QNode::Leaf {
                field: "views".into(),
                lookup: "gte".to_string(),
                value: SqlValue::Int(100),
                negated: false,
            },
        ]),
        QNode::Leaf {
            field: "featured".into(),
            lookup: "exact".to_string(),
            value: SqlValue::Bool(true),
            negated: false,
        },
    ]);
    c.bench_function("compile_q_complex_tree", |b| {
        b.iter(|| {
            let mut values = smallvec::SmallVec::<[SqlValue; 8]>::new();
            let mut w = SqlWriter::new_emit();
            compile_q(
                black_box(&complex_q),
                &mut values,
                black_box(Backend::PostgreSQL),
                &mut w,
            )
        })
    });

    // End-to-end compile (plan hash path)
    let base_node = QueryNode {
        operation: QueryOperation::Select { columns: None },
        table: "posts".into(),
        backend: Backend::PostgreSQL,
        db_alias: None,
        filters: vec![],
        q_filter: Some(complex_q.clone()),
        joins: vec![],
        annotations: vec![],
        group_by: vec![],
        having: vec![],
        order_by: vec![],
        limit: Some(100),
        offset: None,
        distinct: false,
    };

    c.bench_function("compile_full_select_cache_miss", |b| {
        b.iter(|| {
            let mut node = base_node.clone();
            node.limit = Some(black_box(100));
            compile(black_box(&node)).unwrap()
        })
    });

    // Warm cache once, then benchmark hits
    let mut warm = base_node.clone();
    warm.limit = Some(100);
    let _ = compile(&warm).unwrap();

    c.bench_function("compile_full_select_cache_hit", |b| {
        b.iter(|| {
            let mut node = base_node.clone();
            node.limit = Some(black_box(100));
            compile(black_box(&node)).unwrap()
        })
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
