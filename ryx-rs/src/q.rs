use std::collections::HashSet;

use once_cell::sync::Lazy;

use ryx_query::ast::QNode;
use ryx_query::symbols::Symbol;

use crate::into_sql::IntoSqlValue;

/// Set of all known lookups, computed once from the registry.
static KNOWN_LOOKUPS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    ryx_query::lookups::all_lookups().iter().copied().collect()
});

/// Parse a Django-style field key into (column_name, lookup).
///
/// Correctly handles chained transforms: `"created_at__year__gte"` →
/// `("created_at", "year__gte")` — searches from the right for a known lookup.
pub(crate) fn parse_field_lookup(field: &str) -> (String, String) {
    let parts: Vec<&str> = field.split("__").collect();
    if parts.len() < 2 {
        return (field.to_string(), "exact".to_string());
    }
    // Search from the right for the last known lookup
    for i in (1..parts.len()).rev() {
        let candidate = parts[i];
        if KNOWN_LOOKUPS.contains(candidate) {
            let col = parts[..i].join("__");
            let lookup = parts[i..].join("__");
            return (col, lookup);
        }
    }
    // No known lookup found — default to exact
    (parts[0].to_string(), "exact".to_string())
}

/// A composable filter expression — Django-style Q objects.
///
/// Supports boolean algebra: `and`, `or`, `not`.
///
/// # Examples
///
/// ```ignore
/// use ryx::Q;
///
/// // Single condition
/// Q::new("age__gte", 18);
///
/// // OR: email contains gmail OR (age >= 25 AND NOT banned)
/// Q::or(
///     Q::new("email__contains", "gmail.com"),
///     Q::and(
///         Q::new("age__gte", 25),
///         Q::not("is_banned", true),
///     ),
/// );
/// ```
#[derive(Debug, Clone)]
pub enum Q {
    Leaf {
        field: String,
        lookup: String,
        value: ryx_query::ast::SqlValue,
        negated: bool,
    },
    And(Vec<Q>),
    Or(Vec<Q>),
    Not(Box<Q>),
}

impl Q {
    /// Create a leaf condition from a Django-style field key.
    ///
    /// The `field` may include lookups: `"age__gte"`, `"name__contains"`.
    pub fn new(field: &str, value: impl IntoSqlValue) -> Self {
        let (col, lookup) = parse_field_lookup(field);
        Q::Leaf {
            field: col,
            lookup,
            value: value.into_sql_value(),
            negated: false,
        }
    }

    /// Create a leaf condition that will be negated in SQL (`NOT (...)`).
    pub fn not(field: &str, value: impl IntoSqlValue) -> Self {
        let (col, lookup) = parse_field_lookup(field);
        Q::Leaf {
            field: col,
            lookup,
            value: value.into_sql_value(),
            negated: true,
        }
    }

    /// Combine children with AND.
    pub fn and(a: Q, b: Q) -> Self {
        Q::And(vec![a, b])
    }

    /// Combine children with OR.
    pub fn or(a: Q, b: Q) -> Self {
        Q::Or(vec![a, b])
    }

    /// Negate a Q expression.
    pub fn negate(q: Q) -> Self {
        Q::Not(Box::new(q))
    }
}

/// Convert the public `Q` enum into the internal `QNode` enum from `ryx_query`.
pub(crate) fn q_to_qnode(q: Q) -> QNode {
    match q {
        Q::Leaf {
            field,
            lookup,
            value,
            negated,
        } => QNode::Leaf {
            field: Symbol::from(field.as_str()),
            lookup,
            value,
            negated,
        },
        Q::And(children) => {
            QNode::And(children.into_iter().map(q_to_qnode).collect())
        }
        Q::Or(children) => {
            QNode::Or(children.into_iter().map(q_to_qnode).collect())
        }
        Q::Not(child) => QNode::Not(Box::new(q_to_qnode(*child))),
    }
}
