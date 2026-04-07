//
// ###
// Ryx — SQL Compiler
//
// Supports:
//   compile_q()       : recursive Q-tree → SQL fragment
//   compile_joins()   : JoinClause list → SQL JOIN clauses
//   compile_aggs()    : AggregateExpr list → SELECT aggregate columns
//   compile_group_by(): GROUP BY clause
//   compile_having()  : HAVING clause (same engine as WHERE)
//   compile_select()  : now merges plain columns + aggregate annotations
// ###

use crate::errors::{RyxError, RyxResult};
use crate::pool::Backend;
use crate::query::ast::{
    AggFunc, AggregateExpr, FilterNode, JoinClause, JoinKind, QNode, QueryNode, QueryOperation,
    SortDirection, SqlValue,
};
use crate::query::lookup::{self, LookupContext};

// ###
// Output type
// ###
#[derive(Debug, Clone)]
pub struct CompiledQuery {
    pub sql: String,
    pub values: Vec<SqlValue>,
}

// ###
// Public entry point
// ###
pub fn compile(node: &QueryNode) -> RyxResult<CompiledQuery> {
    let mut values: Vec<SqlValue> = Vec::new();
    let sql = match &node.operation {
        QueryOperation::Select { columns } => {
            compile_select(node, columns.as_deref(), &mut values)?
        }
        QueryOperation::Aggregate => compile_aggregate(node, &mut values)?,
        QueryOperation::Count => compile_count(node, &mut values)?,
        QueryOperation::Delete => compile_delete(node, &mut values)?,
        QueryOperation::Update { assignments } => compile_update(node, assignments, &mut values)?,
        QueryOperation::Insert {
            values: cv,
            returning_id,
        } => compile_insert(node, cv, *returning_id, &mut values)?,
    };
    Ok(CompiledQuery { sql, values })
}

// ###
// SELECT
// ###

fn compile_select(
    node: &QueryNode,
    columns: Option<&[String]>,
    values: &mut Vec<SqlValue>,
) -> RyxResult<String> {
    // # SELECT list
    // Columns from plain columns arg + annotation aliases merged together.
    let base_cols = match columns {
        None => "*".to_string(),
        Some(cols) => cols
            .iter()
            .map(|c| qualified_col(c))
            .collect::<Vec<_>>()
            .join(", "),
    };

    let agg_cols = compile_agg_cols(&node.annotations);

    let select_list = match (base_cols.as_str(), agg_cols.as_str()) {
        (_, "") => base_cols,
        ("*", _) => {
            // When we have annotations we drop the bare * and only emit the
            // GROUP BY columns + aggregates (standard SQL).
            if node.group_by.is_empty() {
                agg_cols
            } else {
                let gb = node
                    .group_by
                    .iter()
                    .map(|c| quote_col(c))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{gb}, {agg_cols}")
            }
        }
        (_, _) => format!("{base_cols}, {agg_cols}"),
    };

    let distinct = if node.distinct { "DISTINCT " } else { "" };
    let mut sql = format!(
        "SELECT {distinct}{select_list} FROM {tbl}",
        tbl = quote_col(&node.table),
    );

    // # JOINs
    if !node.joins.is_empty() {
        sql.push(' ');
        sql.push_str(&compile_joins(&node.joins));
    }

    // # WHERE
    let where_sql =
        compile_where_combined(&node.filters, node.q_filter.as_ref(), values, node.backend)?;
    if !where_sql.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&where_sql);
    }

    // # GROUP BY
    if !node.group_by.is_empty() {
        let gb = node
            .group_by
            .iter()
            .map(|c| quote_col(c))
            .collect::<Vec<_>>()
            .join(", ");
        sql.push_str(" GROUP BY ");
        sql.push_str(&gb);
    }

    // # HAVING
    if !node.having.is_empty() {
        let having = compile_filters(&node.having, values, node.backend)?;
        sql.push_str(" HAVING ");
        sql.push_str(&having);
    }

    // # ORDER BY
    if !node.order_by.is_empty() {
        sql.push_str(" ORDER BY ");
        sql.push_str(&compile_order_by(&node.order_by));
    }

    if let Some(n) = node.limit {
        sql.push_str(&format!(" LIMIT {n}"));
    }
    if let Some(n) = node.offset {
        sql.push_str(&format!(" OFFSET {n}"));
    }

    Ok(sql)
}

// ###
// AGGREGATE (no rows returned — only aggregate scalars)
//
// Used by `.aggregate(total=Sum("views"))`.
// Returns a single row dict like {"total": 1234, "avg_views": 42.5}.
// ###
fn compile_aggregate(node: &QueryNode, values: &mut Vec<SqlValue>) -> RyxResult<String> {
    if node.annotations.is_empty() {
        return Err(RyxError::Internal(
            "aggregate() called with no aggregate expressions".into(),
        ));
    }
    let agg_cols = compile_agg_cols(&node.annotations);
    let mut sql = format!("SELECT {agg_cols} FROM {}", quote_col(&node.table));

    if !node.joins.is_empty() {
        sql.push(' ');
        sql.push_str(&compile_joins(&node.joins));
    }

    let where_sql =
        compile_where_combined(&node.filters, node.q_filter.as_ref(), values, node.backend)?;
    if !where_sql.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&where_sql);
    }

    Ok(sql)
}

// ###
// COUNT
// ###

// ###
// COUNT
// ###

fn compile_count(node: &QueryNode, values: &mut Vec<SqlValue>) -> RyxResult<String> {
    let mut sql = format!("SELECT COUNT(*) FROM {}", quote_col(&node.table));
    if !node.joins.is_empty() {
        sql.push(' ');
        sql.push_str(&compile_joins(&node.joins));
    }
    let where_sql =
        compile_where_combined(&node.filters, node.q_filter.as_ref(), values, node.backend)?;
    if !where_sql.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&where_sql);
    }
    Ok(sql)
}

// ###
// DELETE
// ###

fn compile_delete(node: &QueryNode, values: &mut Vec<SqlValue>) -> RyxResult<String> {
    let mut sql = format!("DELETE FROM {}", quote_col(&node.table));
    let where_sql =
        compile_where_combined(&node.filters, node.q_filter.as_ref(), values, node.backend)?;
    if !where_sql.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&where_sql);
    }
    Ok(sql)
}

// ###
// UPDATE
// ###

fn compile_update(
    node: &QueryNode,
    assignments: &[(String, SqlValue)],
    values: &mut Vec<SqlValue>,
) -> RyxResult<String> {
    if assignments.is_empty() {
        return Err(RyxError::Internal("UPDATE with no assignments".into()));
    }
    let set: Vec<String> = assignments
        .iter()
        .map(|(col, val)| {
            values.push(val.clone());
            format!("{} = ?", quote_col(col))
        })
        .collect();
    let mut sql = format!("UPDATE {} SET {}", quote_col(&node.table), set.join(", "));
    let where_sql =
        compile_where_combined(&node.filters, node.q_filter.as_ref(), values, node.backend)?;
    if !where_sql.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&where_sql);
    }
    Ok(sql)
}

// ###
// INSERT
// ###

fn compile_insert(
    node: &QueryNode,
    cols_vals: &[(String, SqlValue)],
    returning_id: bool,
    values: &mut Vec<SqlValue>,
) -> RyxResult<String> {
    if cols_vals.is_empty() {
        return Err(RyxError::Internal("INSERT with no values".into()));
    }
    let (cols, vals): (Vec<_>, Vec<_>) = cols_vals.iter().cloned().unzip();
    values.extend(vals);
    let cols_sql = cols
        .iter()
        .map(|c| quote_col(c))
        .collect::<Vec<_>>()
        .join(", ");
    let ph = std::iter::repeat_n("?", cols.len())
        .collect::<Vec<_>>()
        .join(", ");
    let mut sql = format!(
        "INSERT INTO {} ({}) VALUES ({})",
        quote_col(&node.table),
        cols_sql,
        ph
    );
    if returning_id {
        sql.push_str(" RETURNING id");
    }
    Ok(sql)
}

// ###
// JOIN compilation
// ###

fn compile_joins(joins: &[JoinClause]) -> String {
    joins
        .iter()
        .map(|j| {
            let kind = match j.kind {
                JoinKind::Inner => "INNER JOIN",
                JoinKind::LeftOuter => "LEFT OUTER JOIN",
                JoinKind::RightOuter => "RIGHT OUTER JOIN",
                JoinKind::FullOuter => "FULL OUTER JOIN",
                JoinKind::CrossJoin => "CROSS JOIN",
            };
            let alias_sql = j
                .alias
                .as_deref()
                .map(|a| format!(" AS {}", quote_col(a)))
                .unwrap_or_default();
            let (l_table, l_col) = split_qualified(&j.on_left);
            let (r_table, r_col) = split_qualified(&j.on_right);
            let on_l = if l_table.is_empty() {
                quote_col(&l_col)
            } else {
                format!("{}.{}", quote_col(&l_table), quote_col(&l_col))
            };
            let on_r = if r_table.is_empty() {
                quote_col(&r_col)
            } else {
                format!("{}.{}", quote_col(&r_table), quote_col(&r_col))
            };
            if j.kind == JoinKind::CrossJoin {
                format!("{kind} {}{alias_sql}", quote_col(&j.table))
            } else {
                format!(
                    "{kind} {}{alias_sql} ON {on_l} = {on_r}",
                    quote_col(&j.table)
                )
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// ###
// Aggregate column list  →  SUM("views") AS "total_views", ...
// ###

fn compile_agg_cols(anns: &[AggregateExpr]) -> String {
    anns.iter()
        .map(|a| {
            let col = if a.field == "*" {
                "*".to_string()
            } else {
                qualified_col(&a.field)
            };
            let distinct = if a.distinct && a.func != AggFunc::Count {
                "DISTINCT "
            } else if a.distinct {
                "DISTINCT "
            } else {
                ""
            };
            match &a.func {
                AggFunc::Raw(expr) => format!("{expr} AS {}", quote_col(&a.alias)),
                f => format!(
                    "{}({}{}) AS {}",
                    f.sql_name(),
                    distinct,
                    col,
                    quote_col(&a.alias)
                ),
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

// ###
// WHERE  =  flat filters  AND  Q-tree  (merged)
// ###

fn compile_where_combined(
    filters: &[FilterNode],
    q: Option<&QNode>,
    values: &mut Vec<SqlValue>,
    backend: Backend,
) -> RyxResult<String> {
    let flat = if filters.is_empty() {
        None
    } else {
        Some(compile_filters(filters, values, backend)?)
    };
    let qtree = if let Some(q) = q {
        Some(compile_q(q, values, backend)?)
    } else {
        None
    };
    Ok(match (flat, qtree) {
        (None, None) => String::new(),
        (Some(f), None) => f,
        (None, Some(q)) => q,
        (Some(f), Some(q)) => format!("({f}) AND ({q})"),
    })
}

// ###
// Q-tree compiler  (recursive)
// ###

/// Recursively compile a QNode tree into a SQL fragment.
///
/// Design: we emit minimal parentheses — each non-leaf node wraps its children
/// in parens only when necessary (AND inside OR must be parenthesised).
fn compile_q(q: &QNode, values: &mut Vec<SqlValue>, backend: Backend) -> RyxResult<String> {
    match q {
        QNode::Leaf {
            field,
            lookup,
            value,
            negated,
        } => compile_single_filter(field, lookup, value, *negated, values, backend),
        QNode::And(children) => {
            let parts: Vec<String> = children
                .iter()
                .map(|c| compile_q(c, values, backend))
                .collect::<RyxResult<_>>()?;
            Ok(format!("({})", parts.join(" AND ")))
        }
        QNode::Or(children) => {
            let parts: Vec<String> = children
                .iter()
                .map(|c| compile_q(c, values, backend))
                .collect::<RyxResult<_>>()?;
            Ok(format!("({})", parts.join(" OR ")))
        }
        QNode::Not(child) => {
            let inner = compile_q(child, values, backend)?;
            Ok(format!("NOT ({inner})"))
        }
    }
}

// ###
// Flat filter list compiler
// ###

fn compile_filters(
    filters: &[FilterNode],
    values: &mut Vec<SqlValue>,
    backend: Backend,
) -> RyxResult<String> {
    let parts: Vec<String> = filters
        .iter()
        .map(|f| compile_single_filter(&f.field, &f.lookup, &f.value, f.negated, values, backend))
        .collect::<RyxResult<_>>()?;
    Ok(parts.join(" AND "))
}

// ###
// Single filter → SQL fragment  (shared by flat list and Q-tree)
// ###

fn compile_single_filter(
    field: &str,
    lookup: &str,
    value: &SqlValue,
    negated: bool,
    values: &mut Vec<SqlValue>,
    backend: Backend,
) -> RyxResult<String> {
    // Support "table.column" qualified references in filters
    // Also handle field__transform patterns (e.g., "created_at__year")
    let (base_column, applied_transforms) = if field.contains("__") {
        let parts: Vec<&str> = field.split("__").collect();
        let transforms: Vec<&str> = parts[1..].to_vec();

        // Check if all suffix parts are transforms
        let known_transforms = [
            "date", "year", "month", "day", "hour", "minute", "second", "week", "dow", "quarter",
            "time", "iso_week", "iso_dow", "key", "key_text", "json",
        ];

        // Only treat as transforms if ALL parts after the first are known transforms
        let all_transforms =
            !transforms.is_empty() && transforms.iter().all(|t| known_transforms.contains(t));

        if all_transforms {
            (parts[0].to_string(), transforms)
        } else {
            (field.to_string(), vec![])
        }
    } else {
        (field.to_string(), vec![])
    };

    // If the lookup contains "__" (is a chained lookup like "month__gte"),
    // DON'T apply transforms here - let resolve() handle it completely
    // This avoids double-transform issues where the compiler applies transform
    // and then resolve() also tries to handle it
    let final_column = if lookup.contains("__") {
        // For chained lookups, use just the base column - resolve() will handle transforms
        qualified_col(&base_column)
    } else if !applied_transforms.is_empty() {
        // For simple transform-only lookups (like "year"), apply transforms here
        let mut result = qualified_col(&base_column);
        for transform in &applied_transforms {
            result = lookup::apply_transform(transform, &result, backend)?;
        }
        result
    } else {
        qualified_col(&base_column)
    };

    let ctx = LookupContext {
        column: final_column.clone(),
        negated,
        backend,
    };

    // # isnull (no bind param)
    if lookup == "isnull" {
        let is_null = match value {
            SqlValue::Bool(b) => *b,
            SqlValue::Int(i) => *i != 0,
            _ => true,
        };
        let fragment = if is_null {
            format!("{final_column} IS NULL")
        } else {
            format!("{final_column} IS NOT NULL")
        };
        return Ok(if negated {
            format!("NOT ({fragment})")
        } else {
            fragment
        });
    }

    // # in (expand N placeholders)
    if lookup == "in" {
        let items = match value {
            SqlValue::List(v) => v.clone(),
            other => vec![other.clone()],
        };
        if items.is_empty() {
            return Ok("(1 = 0)".into());
        }
        let ph = std::iter::repeat_n("?", items.len())
            .collect::<Vec<_>>()
            .join(", ");
        values.extend(items);
        let fragment = format!("{final_column} IN ({ph})");
        return Ok(if negated {
            format!("NOT ({fragment})")
        } else {
            fragment
        });
    }

    // # range (two bind params)
    if lookup == "range" {
        let (lo, hi) = match value {
            SqlValue::List(v) if v.len() == 2 => (v[0].clone(), v[1].clone()),
            _ => return Err(RyxError::Internal("range needs exactly 2 values".into())),
        };
        values.push(lo);
        values.push(hi);
        let fragment = format!("{final_column} BETWEEN ? AND ?");
        return Ok(if negated {
            format!("NOT ({fragment})")
        } else {
            fragment
        });
    }

    // # general lookup
    // If lookup is a transform (like "year", "month"), use the transform function which includes = ?
    // BUT if lookup contains "__" (like "date__gte"), we need to use resolve() to handle the chain
    let known_transforms = [
        "date", "year", "month", "day", "hour", "minute", "second", "week", "dow", "quarter",
        "time", "iso_week", "iso_dow", "key", "key_text", "json",
    ];

    // If lookup contains "__", it's a chained lookup (e.g., "date__gte") - use resolve()
    if lookup.contains("__") {
        let fragment = lookup::resolve(&base_column, lookup, &ctx)?;
        values.push(value.clone());
        return Ok(if negated {
            format!("NOT ({fragment})")
        } else {
            fragment
        });
    }

    if known_transforms.contains(&lookup) {
        let transform_fn = match lookup {
            "date" => lookup::date_transform,
            "year" => lookup::year_transform,
            "month" => lookup::month_transform,
            "day" => lookup::day_transform,
            "hour" => lookup::hour_transform,
            "minute" => lookup::minute_transform,
            "second" => lookup::second_transform,
            "week" => lookup::week_transform,
            "dow" => lookup::dow_transform,
            "quarter" => lookup::quarter_transform,
            "time" => lookup::time_transform,
            "iso_week" => lookup::iso_week_transform,
            "iso_dow" => lookup::iso_dow_transform,
            "key" => lookup::json_key_transform,
            "key_text" => lookup::json_key_text_transform,
            "json" => lookup::json_cast_transform,
            _ => {
                return Err(RyxError::UnknownLookup {
                    field: field.to_string(),
                    lookup: lookup.to_string(),
                })
            }
        };
        // For transforms, we need to push the value to the values vector
        values.push(value.clone());
        return Ok(transform_fn(&ctx));
    }

    let fragment = lookup::resolve(&base_column, lookup, &ctx)?;
    let bound = apply_like_wrapping(lookup, value.clone());
    values.push(bound);
    Ok(if negated {
        format!("NOT ({fragment})")
    } else {
        fragment
    })
}

// ###
// ORDER BY
// ###
fn compile_order_by(clauses: &[crate::query::ast::OrderByClause]) -> String {
    clauses
        .iter()
        .map(|c| {
            let dir = match c.direction {
                SortDirection::Asc => "ASC",
                SortDirection::Desc => "DESC",
            };
            format!("{} {dir}", qualified_col(&c.field))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

// ###
// Identifier helpers
// ###

/// Double-quote a simple identifier (column or table name).
pub fn quote_col(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// Handle `table.column` → `"table"."column"`, or plain column → `"column"`.
/// Also handles annotation aliases (already an expression — left as-is).
fn qualified_col(s: &str) -> String {
    if s.contains('.') {
        let (table, col) = s.split_once('.').unwrap();
        format!("{}.{}", quote_col(table), quote_col(col))
    } else {
        quote_col(s)
    }
}

/// Split `"table.column"` into `("table", "column")`.
/// Returns `("", s)` if there is no dot.
fn split_qualified(s: &str) -> (String, String) {
    if let Some((t, c)) = s.split_once('.') {
        (t.to_string(), c.to_string())
    } else {
        (String::new(), s.to_string())
    }
}

/// Apply LIKE `%` wrapping to the value based on the lookup type.
fn apply_like_wrapping(lookup: &str, value: SqlValue) -> SqlValue {
    match lookup {
        "contains" | "icontains" => wrap_text(value, |s| format!("%{s}%")),
        "startswith" | "istartswith" => wrap_text(value, |s| format!("{s}%")),
        "endswith" | "iendswith" => wrap_text(value, |s| format!("%{s}")),
        _ => value,
    }
}

fn wrap_text(value: SqlValue, f: impl Fn(String) -> String) -> SqlValue {
    if let SqlValue::Text(s) = value {
        SqlValue::Text(f(s))
    } else {
        value
    }
}

// ###
// Unit tests
// ###

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::ast::*;
    use crate::query::lookup;

    fn init() {
        lookup::init_registry();
    }

    #[test]
    fn test_bare_select() {
        init();
        let q = compile(&QueryNode::select("posts")).unwrap();
        assert_eq!(q.sql, r#"SELECT * FROM "posts""#);
    }

    #[test]
    fn test_q_or() {
        init();
        let mut node = QueryNode::select("posts");
        node = node.with_q(QNode::Or(vec![
            QNode::Leaf {
                field: "active".into(),
                lookup: "exact".into(),
                value: SqlValue::Bool(true),
                negated: false,
            },
            QNode::Leaf {
                field: "views".into(),
                lookup: "gte".into(),
                value: SqlValue::Int(1000),
                negated: false,
            },
        ]));
        let q = compile(&node).unwrap();
        assert!(q.sql.contains("OR"), "{}", q.sql);
    }

    #[test]
    fn test_inner_join() {
        init();
        let node = QueryNode::select("posts").with_join(JoinClause {
            kind: JoinKind::Inner,
            table: "authors".into(),
            alias: Some("a".into()),
            on_left: "posts.author_id".into(),
            on_right: "a.id".into(),
        });
        let q = compile(&node).unwrap();
        assert!(q.sql.contains("INNER JOIN"), "{}", q.sql);
        assert!(q.sql.contains("ON"), "{}", q.sql);
    }

    #[test]
    fn test_aggregate_sum() {
        init();
        let mut node = QueryNode::select("posts");
        node.operation = QueryOperation::Aggregate;
        node = node.with_annotation(AggregateExpr {
            alias: "total_views".into(),
            func: AggFunc::Sum,
            field: "views".into(),
            distinct: false,
        });
        let q = compile(&node).unwrap();
        assert!(q.sql.contains("SUM"), "{}", q.sql);
        assert!(q.sql.contains("total_views"), "{}", q.sql);
    }

    #[test]
    fn test_group_by() {
        init();
        let mut node = QueryNode::select("posts");
        node = node
            .with_annotation(AggregateExpr {
                alias: "cnt".into(),
                func: AggFunc::Count,
                field: "*".into(),
                distinct: false,
            })
            .with_group_by("status".into());
        let q = compile(&node).unwrap();
        assert!(q.sql.contains("GROUP BY"), "{}", q.sql);
    }

    #[test]
    fn test_having() {
        init();
        let mut node = QueryNode::select("posts");
        node.operation = QueryOperation::Select { columns: None };
        node = node
            .with_annotation(AggregateExpr {
                alias: "cnt".into(),
                func: AggFunc::Count,
                field: "*".into(),
                distinct: false,
            })
            .with_group_by("author_id".into())
            .with_having(FilterNode {
                field: "cnt".into(),
                lookup: "gte".into(),
                value: SqlValue::Int(5),
                negated: false,
            });
        let q = compile(&node).unwrap();
        assert!(q.sql.contains("HAVING"), "{}", q.sql);
    }
}
