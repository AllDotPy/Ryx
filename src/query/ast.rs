//
// ###
// Ryx — Query Abstract Syntax Tree (AST)
//
// Supports the full range of QuerySet features, including filters, joins, aggregates:
//   - Added AggregateExpr  (COUNT, SUM, AVG, MIN, MAX, GROUP BY)
//   - Added JoinClause     (INNER, LEFT, RIGHT, FULL OUTER)
//   - Added QNode          (boolean algebra: AND / OR / NOT trees)
//   - SqlValue::Subquery   (for EXISTS / IN subquery expressions)
//   - QueryNode gets: joins, group_by, having, annotations, q_filter
// ###

use serde::{Deserialize, Serialize};

// ###
// SqlValue — a Python-safe, DB-bindable value
// ###

/// Every value that can appear as a SQL bind parameter.
///
/// We keep this flat and serialisable (serde) so it can cross the PyO3
/// boundary and be stored in the query AST without any Python references.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SqlValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    /// String, datetime, UUID, Decimal — all stored as text and parsed by the driver.
    Text(String),
    /// Used by `__in` and `__range` lookups. The compiler expands it into
    /// multiple bind placeholders.
    List(Vec<SqlValue>),
}

impl SqlValue {
    pub fn type_name(&self) -> &'static str {
        match self {
            SqlValue::Null => "None",
            SqlValue::Bool(_) => "bool",
            SqlValue::Int(_) => "int",
            SqlValue::Float(_) => "float",
            SqlValue::Text(_) => "str",
            SqlValue::List(_) => "list",
        }
    }
}

// ##
// QNode — boolean filter tree (enables OR / NOT)
//
// Django-style Q objects: Q(active=True) | Q(views__gte=100)
//
// The Python side builds a QNode tree by calling Python Q() constructors and
// combining them with | and &.  The Python layer serialises the tree to a
// nested structure and passes it to Rust as a QNode.
// ###

/// A recursive boolean tree of filter conditions.
///
/// - `Leaf`  → a single filter (field, lookup, value, negated).
/// - `And`   → AND of N children (the default for `.filter(a=1, b=2)`).
/// - `Or`    → OR of N children (produced by Q(a=1) | Q(b=2)).
/// - `Not`   → NOT of one child (produced by ~Q(a=1)).
#[derive(Debug, Clone)]
pub enum QNode {
    /// A single filter condition (leaf of the tree).
    Leaf {
        field: String,
        lookup: String,
        value: SqlValue,
        negated: bool,
    },
    /// All children must be true (SQL: A AND B AND C).
    And(Vec<QNode>),
    /// At least one child must be true (SQL: A OR B OR C).
    Or(Vec<QNode>),
    /// Child must be false (SQL: NOT child).
    Not(Box<QNode>),
}

//
// FilterNode — a single flat WHERE condition (legacy, kept for QueryBuilder)
//
#[derive(Debug, Clone)]
pub struct FilterNode {
    pub field: String,
    pub lookup: String,
    pub value: SqlValue,
    /// If true the condition is wrapped in NOT(...). Set by `.exclude()`.
    pub negated: bool,
}

//
// JoinClause
//
/// The kind of SQL JOIN to emit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinKind {
    Inner,
    LeftOuter,
    RightOuter,
    FullOuter,
    CrossJoin,
}

/// A single JOIN clause on the query.
///
/// Example (INNER JOIN):
///   JoinClause {
///     kind:       JoinKind::Inner,
///     table:      "authors",
///     alias:      Some("a"),
///     on_left:    "posts.author_id",
///     on_right:   "a.id",
///   }
/// → INNER JOIN "authors" AS "a" ON "posts"."author_id" = "a"."id"
#[derive(Debug, Clone)]
pub struct JoinClause {
    pub kind: JoinKind,
    /// The table to join.
    pub table: String,
    /// Optional alias for the joined table (used in ON / SELECT columns).
    pub alias: Option<String>,
    /// Left-hand side of the ON condition: "table.column" or just "column".
    pub on_left: String,
    /// Right-hand side of the ON condition.
    pub on_right: String,
}

// ###
// AggregateExpr — column-level aggregation annotations
//
// Used for:
//   Post.objects.annotate(total_views=Sum("views"))
//   Post.objects.aggregate(avg_views=Avg("views"))
// ###

/// The SQL aggregate function to apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AggFunc {
    Count,
    Sum,
    Avg,
    Min,
    Max,
    /// Raw SQL expression — for advanced use cases not covered by the above.
    Raw(String),
}

impl AggFunc {
    /// Return the SQL function name string.
    pub fn sql_name(&self) -> &str {
        match self {
            AggFunc::Count => "COUNT",
            AggFunc::Sum => "SUM",
            AggFunc::Avg => "AVG",
            AggFunc::Min => "MIN",
            AggFunc::Max => "MAX",
            AggFunc::Raw(s) => s.as_str(),
        }
    }
}

/// A single aggregate annotation: alias → aggregate(field).
///
/// Example:
///   AggregateExpr { alias: "total", func: Sum, field: "views", distinct: false }
///   → SUM("views") AS "total"
#[derive(Debug, Clone)]
pub struct AggregateExpr {
    /// The Python-side name (key in the returned dict).
    pub alias: String,
    /// The aggregate function.
    pub func: AggFunc,
    /// The column to aggregate. `"*"` is valid only for COUNT.
    pub field: String,
    /// If true: COUNT(DISTINCT col) / SUM(DISTINCT col).
    pub distinct: bool,
}

//
// OrderByClause
//
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Asc,
    Desc,
}

#[derive(Debug, Clone)]
pub struct OrderByClause {
    pub field: String,
    pub direction: SortDirection,
}

impl OrderByClause {
    /// Parse Django-style `"-field"` → DESC, `"field"` → ASC.
    pub fn parse(s: &str) -> Self {
        if let Some(f) = s.strip_prefix('-') {
            Self {
                field: f.to_string(),
                direction: SortDirection::Desc,
            }
        } else {
            Self {
                field: s.to_string(),
                direction: SortDirection::Asc,
            }
        }
    }
}

//
// QueryOperation
//
#[derive(Debug, Clone)]
pub enum QueryOperation {
    /// Regular SELECT — returns rows.
    Select {
        /// None → SELECT *. Some(cols) → SELECT col1, col2, ...
        columns: Option<Vec<String>>,
    },
    /// Aggregate-only SELECT — returns a single row of aggregated values.
    /// Used by `.aggregate(total=Sum("views"))`.
    Aggregate,
    /// SELECT COUNT(*) — returns a single integer.
    Count,
    Delete,
    Update {
        assignments: Vec<(String, SqlValue)>,
    },
    Insert {
        values: Vec<(String, SqlValue)>,
        returning_id: bool,
    },
}

//
// QueryNode — the complete query AST
//
/// The complete query AST. Produced by the Python QuerySet and consumed by the
/// SQL compiler.
///
/// Supports also:
///   - `q_filter`    : optional Q-tree for OR/NOT conditions (OR of flat filters)
///   - `joins`       : JOIN clauses for related-table queries
///   - `annotations` : aggregate expressions for annotate() / aggregate()
///   - `group_by`    : GROUP BY columns
///   - `having`      : HAVING conditions (flat list, AND-ed, same as filters)
#[derive(Debug, Clone)]
pub struct QueryNode {
    pub table: String,
    pub operation: QueryOperation,

    // # WHERE
    /// Flat AND-chained filter conditions (from `.filter()` / `.exclude()`).
    /// These are always AND-ed with each other and with `q_filter`.
    pub filters: Vec<FilterNode>,
    /// Optional Q-tree for complex OR/NOT conditions. AND-ed with `filters`.
    pub q_filter: Option<QNode>,

    // # JOINs
    pub joins: Vec<JoinClause>,

    // # Aggregations
    /// Aggregate expressions added by `.annotate()` or `.aggregate()`.
    pub annotations: Vec<AggregateExpr>,
    /// GROUP BY columns (from `.values("field")` combined with aggregate).
    pub group_by: Vec<String>,
    /// HAVING conditions — same format as filters, applied after GROUP BY.
    pub having: Vec<FilterNode>,

    // #  Ordering / paging
    pub order_by: Vec<OrderByClause>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
    pub distinct: bool,
}

impl QueryNode {
    /// Base SELECT * for a table. Starting point for every QuerySet.
    pub fn select(table: impl Into<String>) -> Self {
        Self {
            table: table.into(),
            operation: QueryOperation::Select { columns: None },
            filters: Vec::new(),
            q_filter: None,
            joins: Vec::new(),
            annotations: Vec::new(),
            group_by: Vec::new(),
            having: Vec::new(),
            order_by: Vec::new(),
            limit: None,
            offset: None,
            distinct: false,
        }
    }

    pub fn count(table: impl Into<String>) -> Self {
        let mut n = Self::select(table);
        n.operation = QueryOperation::Count;
        n
    }

    pub fn delete(table: impl Into<String>) -> Self {
        let mut n = Self::select(table);
        n.operation = QueryOperation::Delete;
        n
    }

    // Builder methods (all return a new node — immutable style)

    #[must_use]
    pub fn with_filter(mut self, node: FilterNode) -> Self {
        self.filters.push(node);
        self
    }

    #[must_use]
    pub fn with_q(mut self, q: QNode) -> Self {
        self.q_filter = Some(match self.q_filter.take() {
            None => q,
            Some(prev) => QNode::And(vec![prev, q]),
        });
        self
    }

    #[must_use]
    pub fn with_join(mut self, j: JoinClause) -> Self {
        self.joins.push(j);
        self
    }

    #[must_use]
    pub fn with_annotation(mut self, agg: AggregateExpr) -> Self {
        self.annotations.push(agg);
        self
    }

    #[must_use]
    pub fn with_group_by(mut self, field: String) -> Self {
        self.group_by.push(field);
        self
    }

    #[must_use]
    pub fn with_having(mut self, f: FilterNode) -> Self {
        self.having.push(f);
        self
    }

    #[must_use]
    pub fn with_order_by(mut self, c: OrderByClause) -> Self {
        self.order_by.push(c);
        self
    }

    #[must_use]
    pub fn with_limit(mut self, n: u64) -> Self {
        self.limit = Some(n);
        self
    }

    #[must_use]
    pub fn with_offset(mut self, n: u64) -> Self {
        self.offset = Some(n);
        self
    }
}
