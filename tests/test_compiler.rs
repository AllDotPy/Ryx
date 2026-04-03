// tests/test_compiler_v2.rs
//
// ──────────────────────────────────────────────────────────────────────────────
// Bitya — Rust compiler v2 tests
//
// Covers: JOINs, Q-trees, aggregations, GROUP BY, HAVING, UPDATE, INSERT,
//         DELETE, DISTINCT, complex filter chains, custom lookups.
//
// Run with: cargo test
// ──────────────────────────────────────────────────────────────────────────────

use ryx_core::query::{
    ast::{
        AggFunc, AggregateExpr, FilterNode, JoinClause, JoinKind,
        OrderByClause, QNode, QueryNode, QueryOperation, SqlValue,
    },
    compiler::compile,
    lookup,
};

fn init() {
    lookup::init_registry();
}

// ─── SELECT ──────────────────────────────────────────────────────────────────

#[test]
fn test_select_star() {
    init();
    let q = compile(&QueryNode::select("posts")).unwrap();
    assert_eq!(q.sql, r#"SELECT * FROM "posts""#);
    assert!(q.values.is_empty());
}

#[test]
fn test_select_distinct() {
    init();
    let mut n = QueryNode::select("tags");
    n.distinct = true;
    let q = compile(&n).unwrap();
    assert!(q.sql.contains("SELECT DISTINCT"), "{}", q.sql);
}

#[test]
fn test_select_with_limit_offset() {
    init();
    let q = compile(&QueryNode::select("posts").with_limit(10).with_offset(20)).unwrap();
    assert!(q.sql.contains("LIMIT 10"),  "{}", q.sql);
    assert!(q.sql.contains("OFFSET 20"), "{}", q.sql);
}

// ─── WHERE / filters ─────────────────────────────────────────────────────────

#[test]
fn test_exact_filter() {
    init();
    let q = compile(&QueryNode::select("users").with_filter(FilterNode {
        field: "email".into(), lookup: "exact".into(),
        value: SqlValue::Text("alice@example.com".into()), negated: false,
    })).unwrap();
    assert!(q.sql.contains(r#""email" = ?"#), "{}", q.sql);
    assert_eq!(q.values.len(), 1);
}

#[test]
fn test_multiple_filters_anded() {
    init();
    let q = compile(&QueryNode::select("posts")
        .with_filter(FilterNode { field: "active".into(), lookup: "exact".into(),
            value: SqlValue::Bool(true), negated: false })
        .with_filter(FilterNode { field: "views".into(), lookup: "gte".into(),
            value: SqlValue::Int(100), negated: false })
    ).unwrap();
    assert!(q.sql.contains("AND"), "{}", q.sql);
    assert_eq!(q.values.len(), 2);
}

#[test]
fn test_negated_filter() {
    init();
    let q = compile(&QueryNode::select("posts").with_filter(FilterNode {
        field: "status".into(), lookup: "exact".into(),
        value: SqlValue::Text("draft".into()), negated: true,
    })).unwrap();
    assert!(q.sql.contains("NOT ("), "{}", q.sql);
}

#[test]
fn test_isnull_true() {
    init();
    let q = compile(&QueryNode::select("posts").with_filter(FilterNode {
        field: "deleted_at".into(), lookup: "isnull".into(),
        value: SqlValue::Bool(true), negated: false,
    })).unwrap();
    assert!(q.sql.contains("IS NULL"), "{}", q.sql);
    assert!(q.values.is_empty(), "isnull binds no value");
}

#[test]
fn test_isnull_false() {
    init();
    let q = compile(&QueryNode::select("posts").with_filter(FilterNode {
        field: "deleted_at".into(), lookup: "isnull".into(),
        value: SqlValue::Bool(false), negated: false,
    })).unwrap();
    assert!(q.sql.contains("IS NOT NULL"), "{}", q.sql);
    assert!(q.values.is_empty());
}

#[test]
fn test_in_lookup_expands() {
    init();
    let q = compile(&QueryNode::select("users").with_filter(FilterNode {
        field: "id".into(), lookup: "in".into(),
        value: SqlValue::List(vec![SqlValue::Int(1), SqlValue::Int(2), SqlValue::Int(3)]),
        negated: false,
    })).unwrap();
    assert!(q.sql.contains("IN (?, ?, ?)"), "{}", q.sql);
    assert_eq!(q.values.len(), 3);
}

#[test]
fn test_empty_in_produces_false() {
    init();
    let q = compile(&QueryNode::select("users").with_filter(FilterNode {
        field: "id".into(), lookup: "in".into(),
        value: SqlValue::List(vec![]), negated: false,
    })).unwrap();
    assert!(q.sql.contains("1 = 0"), "{}", q.sql);
    assert!(q.values.is_empty());
}

#[test]
fn test_range_uses_between() {
    init();
    let q = compile(&QueryNode::select("posts").with_filter(FilterNode {
        field: "views".into(), lookup: "range".into(),
        value: SqlValue::List(vec![SqlValue::Int(10), SqlValue::Int(100)]),
        negated: false,
    })).unwrap();
    assert!(q.sql.contains("BETWEEN ? AND ?"), "{}", q.sql);
    assert_eq!(q.values.len(), 2);
}

#[test]
fn test_contains_wraps_percent() {
    init();
    let q = compile(&QueryNode::select("posts").with_filter(FilterNode {
        field: "title".into(), lookup: "contains".into(),
        value: SqlValue::Text("rust".into()), negated: false,
    })).unwrap();
    match &q.values[0] {
        SqlValue::Text(s) => assert_eq!(s, "%rust%"),
        v => panic!("expected Text, got {:?}", v),
    }
}

#[test]
fn test_startswith_appends_percent() {
    init();
    let q = compile(&QueryNode::select("posts").with_filter(FilterNode {
        field: "title".into(), lookup: "startswith".into(),
        value: SqlValue::Text("Hello".into()), negated: false,
    })).unwrap();
    match &q.values[0] {
        SqlValue::Text(s) => assert_eq!(s, "Hello%"),
        v => panic!("{:?}", v),
    }
}

#[test]
fn test_endswith_prepends_percent() {
    init();
    let q = compile(&QueryNode::select("posts").with_filter(FilterNode {
        field: "title".into(), lookup: "endswith".into(),
        value: SqlValue::Text("World".into()), negated: false,
    })).unwrap();
    match &q.values[0] {
        SqlValue::Text(s) => assert_eq!(s, "%World"),
        v => panic!("{:?}", v),
    }
}

#[test]
fn test_icontains_uses_lower() {
    init();
    let q = compile(&QueryNode::select("users").with_filter(FilterNode {
        field: "name".into(), lookup: "icontains".into(),
        value: SqlValue::Text("alice".into()), negated: false,
    })).unwrap();
    assert!(q.sql.to_uppercase().contains("LOWER"), "{}", q.sql);
}

// ─── Q-tree ──────────────────────────────────────────────────────────────────

#[test]
fn test_q_or_tree() {
    init();
    let q_node = QNode::Or(vec![
        QNode::Leaf { field: "active".into(), lookup: "exact".into(),
            value: SqlValue::Bool(true), negated: false },
        QNode::Leaf { field: "views".into(), lookup: "gte".into(),
            value: SqlValue::Int(1000), negated: false },
    ]);
    let node = QueryNode::select("posts").with_q(q_node);
    let q = compile(&node).unwrap();
    assert!(q.sql.contains("OR"), "Q OR should produce OR: {}", q.sql);
    assert_eq!(q.values.len(), 2);
}

#[test]
fn test_q_and_tree() {
    init();
    let q_node = QNode::And(vec![
        QNode::Leaf { field: "active".into(), lookup: "exact".into(),
            value: SqlValue::Bool(true), negated: false },
        QNode::Leaf { field: "verified".into(), lookup: "exact".into(),
            value: SqlValue::Bool(true), negated: false },
    ]);
    let q = compile(&QueryNode::select("users").with_q(q_node)).unwrap();
    assert!(q.sql.contains("AND"), "{}", q.sql);
}

#[test]
fn test_q_not_tree() {
    init();
    let q_node = QNode::Not(Box::new(
        QNode::Leaf { field: "status".into(), lookup: "exact".into(),
            value: SqlValue::Text("draft".into()), negated: false }
    ));
    let q = compile(&QueryNode::select("posts").with_q(q_node)).unwrap();
    assert!(q.sql.contains("NOT"), "{}", q.sql);
}

#[test]
fn test_q_tree_combined_with_flat_filters() {
    init();
    let q_node = QNode::Or(vec![
        QNode::Leaf { field: "premium".into(), lookup: "exact".into(),
            value: SqlValue::Bool(true), negated: false },
        QNode::Leaf { field: "views".into(), lookup: "gte".into(),
            value: SqlValue::Int(500), negated: false },
    ]);
    let node = QueryNode::select("posts")
        .with_filter(FilterNode { field: "active".into(), lookup: "exact".into(),
            value: SqlValue::Bool(true), negated: false })
        .with_q(q_node);
    let q = compile(&node).unwrap();
    // Flat filters AND Q-tree are combined with AND
    assert!(q.sql.contains("AND"), "{}", q.sql);
    assert!(q.sql.contains("OR"),  "{}", q.sql);
    assert_eq!(q.values.len(), 3);
}

#[test]
fn test_nested_q_tree() {
    init();
    // (A OR B) AND (C OR D)
    let q_node = QNode::And(vec![
        QNode::Or(vec![
            QNode::Leaf { field: "a".into(), lookup: "exact".into(),
                value: SqlValue::Int(1), negated: false },
            QNode::Leaf { field: "b".into(), lookup: "exact".into(),
                value: SqlValue::Int(2), negated: false },
        ]),
        QNode::Or(vec![
            QNode::Leaf { field: "c".into(), lookup: "exact".into(),
                value: SqlValue::Int(3), negated: false },
            QNode::Leaf { field: "d".into(), lookup: "exact".into(),
                value: SqlValue::Int(4), negated: false },
        ]),
    ]);
    let q = compile(&QueryNode::select("t").with_q(q_node)).unwrap();
    assert_eq!(q.values.len(), 4);
    // Should contain both OR and AND
    assert!(q.sql.contains("OR"),  "{}", q.sql);
    assert!(q.sql.contains("AND"), "{}", q.sql);
}

// ─── JOINs ───────────────────────────────────────────────────────────────────

#[test]
fn test_inner_join() {
    init();
    let node = QueryNode::select("posts").with_join(JoinClause {
        kind: JoinKind::Inner, table: "authors".into(),
        alias: Some("a".into()),
        on_left: "posts.author_id".into(), on_right: "a.id".into(),
    });
    let q = compile(&node).unwrap();
    assert!(q.sql.contains("INNER JOIN"), "{}", q.sql);
    assert!(q.sql.contains("ON"), "{}", q.sql);
}

#[test]
fn test_left_outer_join() {
    init();
    let node = QueryNode::select("posts").with_join(JoinClause {
        kind: JoinKind::LeftOuter, table: "comments".into(),
        alias: None,
        on_left: "posts.id".into(), on_right: "comments.post_id".into(),
    });
    let q = compile(&node).unwrap();
    assert!(q.sql.contains("LEFT OUTER JOIN"), "{}", q.sql);
}

#[test]
fn test_cross_join() {
    init();
    let node = QueryNode::select("a").with_join(JoinClause {
        kind: JoinKind::CrossJoin, table: "b".into(),
        alias: None, on_left: String::new(), on_right: String::new(),
    });
    let q = compile(&node).unwrap();
    assert!(q.sql.contains("CROSS JOIN"), "{}", q.sql);
}

#[test]
fn test_join_with_filter() {
    init();
    let node = QueryNode::select("posts")
        .with_join(JoinClause {
            kind: JoinKind::Inner, table: "authors".into(),
            alias: Some("a".into()),
            on_left: "posts.author_id".into(), on_right: "a.id".into(),
        })
        .with_filter(FilterNode {
            field: "a.verified".into(), lookup: "exact".into(),
            value: SqlValue::Bool(true), negated: false,
        });
    let q = compile(&node).unwrap();
    assert!(q.sql.contains("INNER JOIN"), "{}", q.sql);
    assert!(q.sql.contains("WHERE"), "{}", q.sql);
    assert_eq!(q.values.len(), 1);
}

// ─── Aggregations ─────────────────────────────────────────────────────────────

#[test]
fn test_aggregate_count() {
    init();
    let mut node = QueryNode::select("posts");
    node.operation = QueryOperation::Aggregate;
    node = node.with_annotation(AggregateExpr {
        alias: "cnt".into(), func: AggFunc::Count,
        field: "*".into(), distinct: false,
    });
    let q = compile(&node).unwrap();
    assert!(q.sql.contains("COUNT(*)"), "{}", q.sql);
    assert!(q.sql.contains("cnt"), "{}", q.sql);
}

#[test]
fn test_aggregate_sum() {
    init();
    let mut node = QueryNode::select("orders");
    node.operation = QueryOperation::Aggregate;
    node = node.with_annotation(AggregateExpr {
        alias: "total".into(), func: AggFunc::Sum,
        field: "amount".into(), distinct: false,
    });
    let q = compile(&node).unwrap();
    assert!(q.sql.contains("SUM"), "{}", q.sql);
    assert!(q.sql.contains("total"), "{}", q.sql);
}

#[test]
fn test_aggregate_avg() {
    init();
    let mut node = QueryNode::select("products");
    node.operation = QueryOperation::Aggregate;
    node = node.with_annotation(AggregateExpr {
        alias: "avg_price".into(), func: AggFunc::Avg,
        field: "price".into(), distinct: false,
    });
    let q = compile(&node).unwrap();
    assert!(q.sql.contains("AVG"), "{}", q.sql);
}

#[test]
fn test_aggregate_min_max() {
    init();
    let mut node = QueryNode::select("products");
    node.operation = QueryOperation::Aggregate;
    node = node
        .with_annotation(AggregateExpr {
            alias: "min_p".into(), func: AggFunc::Min,
            field: "price".into(), distinct: false,
        })
        .with_annotation(AggregateExpr {
            alias: "max_p".into(), func: AggFunc::Max,
            field: "price".into(), distinct: false,
        });
    let q = compile(&node).unwrap();
    assert!(q.sql.contains("MIN"), "{}", q.sql);
    assert!(q.sql.contains("MAX"), "{}", q.sql);
}

#[test]
fn test_count_distinct() {
    init();
    let mut node = QueryNode::select("orders");
    node.operation = QueryOperation::Aggregate;
    node = node.with_annotation(AggregateExpr {
        alias: "unique_users".into(), func: AggFunc::Count,
        field: "user_id".into(), distinct: true,
    });
    let q = compile(&node).unwrap();
    assert!(q.sql.contains("DISTINCT"), "{}", q.sql);
}

#[test]
fn test_annotate_with_group_by() {
    init();
    let node = QueryNode::select("posts")
        .with_annotation(AggregateExpr {
            alias: "cnt".into(), func: AggFunc::Count,
            field: "*".into(), distinct: false,
        })
        .with_group_by("author_id".into());
    let q = compile(&node).unwrap();
    assert!(q.sql.contains("GROUP BY"), "{}", q.sql);
    assert!(q.sql.contains("author_id"), "{}", q.sql);
}

#[test]
fn test_having_clause() {
    init();
    let node = QueryNode::select("posts")
        .with_annotation(AggregateExpr {
            alias: "cnt".into(), func: AggFunc::Count,
            field: "*".into(), distinct: false,
        })
        .with_group_by("author_id".into())
        .with_having(FilterNode {
            field: "cnt".into(), lookup: "gte".into(),
            value: SqlValue::Int(5), negated: false,
        });
    let q = compile(&node).unwrap();
    assert!(q.sql.contains("GROUP BY"), "{}", q.sql);
    assert!(q.sql.contains("HAVING"),   "{}", q.sql);
}

// ─── ORDER BY ────────────────────────────────────────────────────────────────

#[test]
fn test_order_asc() {
    init();
    let q = compile(&QueryNode::select("posts")
        .with_order_by(OrderByClause::parse("title"))).unwrap();
    assert!(q.sql.contains(r#""title" ASC"#), "{}", q.sql);
}

#[test]
fn test_order_desc() {
    init();
    let q = compile(&QueryNode::select("posts")
        .with_order_by(OrderByClause::parse("-views"))).unwrap();
    assert!(q.sql.contains(r#""views" DESC"#), "{}", q.sql);
}

#[test]
fn test_order_multiple() {
    init();
    let q = compile(&QueryNode::select("posts")
        .with_order_by(OrderByClause::parse("-views"))
        .with_order_by(OrderByClause::parse("title"))).unwrap();
    assert!(q.sql.contains(r#""views" DESC, "title" ASC"#), "{}", q.sql);
}

// ─── COUNT ────────────────────────────────────────────────────────────────────

#[test]
fn test_count_star() {
    init();
    let q = compile(&QueryNode::count("users")).unwrap();
    assert_eq!(q.sql, r#"SELECT COUNT(*) FROM "users""#);
    assert!(q.values.is_empty());
}

#[test]
fn test_count_with_filter() {
    init();
    let node = QueryNode::count("users").with_filter(FilterNode {
        field: "active".into(), lookup: "exact".into(),
        value: SqlValue::Bool(true), negated: false,
    });
    let q = compile(&node).unwrap();
    assert!(q.sql.starts_with("SELECT COUNT(*)"), "{}", q.sql);
    assert!(q.sql.contains("WHERE"), "{}", q.sql);
    assert_eq!(q.values.len(), 1);
}

// ─── DELETE ──────────────────────────────────────────────────────────────────

#[test]
fn test_delete_with_filter() {
    init();
    let node = QueryNode::delete("posts").with_filter(FilterNode {
        field: "status".into(), lookup: "exact".into(),
        value: SqlValue::Text("draft".into()), negated: false,
    });
    let q = compile(&node).unwrap();
    assert!(q.sql.starts_with(r#"DELETE FROM "posts""#), "{}", q.sql);
    assert!(q.sql.contains("WHERE"), "{}", q.sql);
}

#[test]
fn test_delete_all_no_where() {
    init();
    let q = compile(&QueryNode::delete("sessions")).unwrap();
    assert!(!q.sql.contains("WHERE"), "Delete all should have no WHERE: {}", q.sql);
}

// ─── UPDATE ──────────────────────────────────────────────────────────────────

#[test]
fn test_update_single_field() {
    init();
    let mut node = QueryNode::select("posts").with_filter(FilterNode {
        field: "id".into(), lookup: "exact".into(),
        value: SqlValue::Int(42), negated: false,
    });
    node.operation = QueryOperation::Update {
        assignments: vec![("views".to_string(), SqlValue::Int(100))],
    };
    let q = compile(&node).unwrap();
    assert!(q.sql.starts_with(r#"UPDATE "posts""#), "{}", q.sql);
    assert!(q.sql.contains("SET"), "{}", q.sql);
    assert!(q.sql.contains("WHERE"), "{}", q.sql);
    assert_eq!(q.values.len(), 2); // 1 SET + 1 WHERE
}

#[test]
fn test_update_multiple_fields() {
    init();
    let mut node = QueryNode::select("users").with_filter(FilterNode {
        field: "id".into(), lookup: "exact".into(),
        value: SqlValue::Int(1), negated: false,
    });
    node.operation = QueryOperation::Update {
        assignments: vec![
            ("name".to_string(),  SqlValue::Text("Bob".into())),
            ("email".to_string(), SqlValue::Text("bob@ex.com".into())),
        ],
    };
    let q = compile(&node).unwrap();
    assert_eq!(q.values.len(), 3); // 2 SET + 1 WHERE
}

// ─── INSERT ──────────────────────────────────────────────────────────────────

#[test]
fn test_insert_basic() {
    init();
    let mut node = QueryNode::select("posts");
    node.operation = QueryOperation::Insert {
        values: vec![
            ("title".to_string(), SqlValue::Text("Hello".into())),
            ("views".to_string(), SqlValue::Int(0)),
        ],
        returning_id: false,
    };
    let q = compile(&node).unwrap();
    assert!(q.sql.starts_with(r#"INSERT INTO "posts""#), "{}", q.sql);
    assert!(q.sql.contains("VALUES (?, ?)"), "{}", q.sql);
    assert_eq!(q.values.len(), 2);
}

#[test]
fn test_insert_returning_id() {
    init();
    let mut node = QueryNode::select("users");
    node.operation = QueryOperation::Insert {
        values: vec![("name".to_string(), SqlValue::Text("Alice".into()))],
        returning_id: true,
    };
    let q = compile(&node).unwrap();
    assert!(q.sql.contains("RETURNING id"), "{}", q.sql);
}

// ─── Custom lookups ──────────────────────────────────────────────────────────

#[test]
fn test_custom_lookup_ilike() {
    init();
    lookup::register_custom("ilike", "{col} ILIKE ?").unwrap();
    let node = QueryNode::select("posts").with_filter(FilterNode {
        field: "title".into(), lookup: "ilike".into(),
        value: SqlValue::Text("hello".into()), negated: false,
    });
    let q = compile(&node).unwrap();
    assert!(q.sql.contains("ILIKE"), "{}", q.sql);
}

#[test]
fn test_custom_lookup_tsearch() {
    init();
    lookup::register_custom(
        "tsearch",
        "to_tsvector('english', {col}) @@ plainto_tsquery(?)",
    ).unwrap();
    let node = QueryNode::select("articles").with_filter(FilterNode {
        field: "body".into(), lookup: "tsearch".into(),
        value: SqlValue::Text("rust programming".into()), negated: false,
    });
    let q = compile(&node).unwrap();
    assert!(q.sql.contains("to_tsvector"), "{}", q.sql);
    assert!(q.sql.contains("plainto_tsquery"), "{}", q.sql);
}

#[test]
fn test_unknown_lookup_errors() {
    init();
    let node = QueryNode::select("posts").with_filter(FilterNode {
        field: "title".into(), lookup: "nonexistent_xyz_lookup".into(),
        value: SqlValue::Text("x".into()), negated: false,
    });
    let result = compile(&node);
    assert!(result.is_err(), "Unknown lookup must return Err");
    assert!(result.unwrap_err().to_string().contains("nonexistent_xyz_lookup"));
}

// ─── Identifier quoting ───────────────────────────────────────────────────────

#[test]
fn test_reserved_word_table_quoted() {
    init();
    let q = compile(&QueryNode::select("order")).unwrap();
    assert!(q.sql.contains(r#""order""#), "{}", q.sql);
}

#[test]
fn test_reserved_word_column_quoted() {
    init();
    let q = compile(&QueryNode::select("t").with_filter(FilterNode {
        field: "select".into(), lookup: "exact".into(),
        value: SqlValue::Int(1), negated: false,
    })).unwrap();
    assert!(q.sql.contains(r#""select""#), "{}", q.sql);
}

// ─── Complex combined queries ─────────────────────────────────────────────────

#[test]
fn test_complex_select_all_clauses() {
    init();
    let node = QueryNode::select("posts")
        .with_join(JoinClause {
            kind: JoinKind::LeftOuter, table: "authors".into(),
            alias: Some("a".into()),
            on_left: "posts.author_id".into(), on_right: "a.id".into(),
        })
        .with_filter(FilterNode {
            field: "posts.active".into(), lookup: "exact".into(),
            value: SqlValue::Bool(true), negated: false,
        })
        .with_q(QNode::Or(vec![
            QNode::Leaf { field: "posts.views".into(), lookup: "gte".into(),
                value: SqlValue::Int(100), negated: false },
            QNode::Leaf { field: "a.verified".into(), lookup: "exact".into(),
                value: SqlValue::Bool(true), negated: false },
        ]))
        .with_annotation(AggregateExpr {
            alias: "cnt".into(), func: AggFunc::Count,
            field: "*".into(), distinct: false,
        })
        .with_group_by("posts.author_id".into())
        .with_having(FilterNode {
            field: "cnt".into(), lookup: "gte".into(),
            value: SqlValue::Int(3), negated: false,
        })
        .with_order_by(OrderByClause::parse("-cnt"))
        .with_limit(10);

    let q = compile(&node).unwrap();
    assert!(q.sql.contains("LEFT OUTER JOIN"), "{}", q.sql);
    assert!(q.sql.contains("WHERE"), "{}", q.sql);
    assert!(q.sql.contains("OR"),    "{}", q.sql);
    assert!(q.sql.contains("GROUP BY"), "{}", q.sql);
    assert!(q.sql.contains("HAVING"),   "{}", q.sql);
    assert!(q.sql.contains("ORDER BY"), "{}", q.sql);
    assert!(q.sql.contains("LIMIT 10"), "{}", q.sql);
}