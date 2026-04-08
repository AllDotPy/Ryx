//
// ###
// Ryx — SQL Compiler Implementation
// ###
//
// This file contains the SQL compiler that transforms QueryNode AST into SQL strings.
// See compiler/mod.rs for the module structure.
// ###

use crate::ast::{
    AggFunc, AggregateExpr, FilterNode, JoinClause, JoinKind, QNode, QueryNode, QueryOperation,
    SortDirection, SqlValue,
};
use crate::backend::Backend;
use crate::errors::{QueryError, QueryResult};
use crate::lookups::date_lookups as date;
use crate::lookups::json_lookups as json;
use crate::lookups::{self, LookupContext};

pub use super::helpers::{apply_like_wrapping, qualified_col, split_qualified, KNOWN_TRANSFORMS};

use super::helpers;

#[derive(Debug, Clone)]
pub struct CompiledQuery {
    pub sql: String,
    pub values: Vec<SqlValue>,
}

pub fn compile(node: &QueryNode) -> QueryResult<CompiledQuery> {
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

fn compile_select(
    node: &QueryNode,
    columns: Option<&[String]>,
    values: &mut Vec<SqlValue>,
) -> QueryResult<String> {
    let base_cols = match columns {
        None => "*".to_string(),
        Some(cols) => cols
            .iter()
            .map(|c| helpers::qualified_col(c))
            .collect::<Vec<_>>()
            .join(", "),
    };

    let agg_cols = compile_agg_cols(&node.annotations);

    let select_list = match (base_cols.as_str(), agg_cols.as_str()) {
        (_, "") => base_cols,
        ("*", _) => {
            if node.group_by.is_empty() {
                agg_cols
            } else {
                let gb = node
                    .group_by
                    .iter()
                    .map(|c| helpers::quote_col(c))
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
        tbl = helpers::quote_col(&node.table),
    );

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

    if !node.group_by.is_empty() {
        let gb = node
            .group_by
            .iter()
            .map(|c| helpers::quote_col(c))
            .collect::<Vec<_>>()
            .join(", ");
        sql.push_str(" GROUP BY ");
        sql.push_str(&gb);
    }

    if !node.having.is_empty() {
        let having = compile_filters(&node.having, values, node.backend)?;
        sql.push_str(" HAVING ");
        sql.push_str(&having);
    }

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

fn compile_aggregate(node: &QueryNode, values: &mut Vec<SqlValue>) -> QueryResult<String> {
    if node.annotations.is_empty() {
        return Err(QueryError::Internal(
            "aggregate() called with no aggregate expressions".into(),
        ));
    }
    let agg_cols = compile_agg_cols(&node.annotations);
    let mut sql = format!("SELECT {agg_cols} FROM {}", helpers::quote_col(&node.table));

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

fn compile_count(node: &QueryNode, values: &mut Vec<SqlValue>) -> QueryResult<String> {
    let mut sql = format!("SELECT COUNT(*) FROM {}", helpers::quote_col(&node.table));
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

fn compile_delete(node: &QueryNode, values: &mut Vec<SqlValue>) -> QueryResult<String> {
    let mut sql = format!("DELETE FROM {}", helpers::quote_col(&node.table));
    let where_sql =
        compile_where_combined(&node.filters, node.q_filter.as_ref(), values, node.backend)?;
    if !where_sql.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&where_sql);
    }
    Ok(sql)
}

fn compile_update(
    node: &QueryNode,
    assignments: &[(String, SqlValue)],
    values: &mut Vec<SqlValue>,
) -> QueryResult<String> {
    if assignments.is_empty() {
        return Err(QueryError::Internal("UPDATE with no assignments".into()));
    }
    let set: Vec<String> = assignments
        .iter()
        .map(|(col, val)| {
            values.push(val.clone());
            format!("{} = ?", helpers::quote_col(col))
        })
        .collect();
    let mut sql = format!(
        "UPDATE {} SET {}",
        helpers::quote_col(&node.table),
        set.join(", ")
    );
    let where_sql =
        compile_where_combined(&node.filters, node.q_filter.as_ref(), values, node.backend)?;
    if !where_sql.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&where_sql);
    }
    Ok(sql)
}

fn compile_insert(
    node: &QueryNode,
    cols_vals: &[(String, SqlValue)],
    returning_id: bool,
    values: &mut Vec<SqlValue>,
) -> QueryResult<String> {
    if cols_vals.is_empty() {
        return Err(QueryError::Internal("INSERT with no values".into()));
    }
    let (cols, vals): (Vec<_>, Vec<_>) = cols_vals.iter().cloned().unzip();
    values.extend(vals);
    let cols_sql = cols
        .iter()
        .map(|c| helpers::quote_col(c))
        .collect::<Vec<_>>()
        .join(", ");
    let ph = std::iter::repeat_n("?", cols.len())
        .collect::<Vec<_>>()
        .join(", ");
    let mut sql = format!(
        "INSERT INTO {} ({}) VALUES ({})",
        helpers::quote_col(&node.table),
        cols_sql,
        ph
    );
    if returning_id {
        sql.push_str(" RETURNING id");
    }
    Ok(sql)
}

pub fn compile_joins(joins: &[JoinClause]) -> String {
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
                .map(|a| format!(" AS {}", helpers::quote_col(a)))
                .unwrap_or_default();
            let (l_table, l_col): (String, String) = helpers::split_qualified(&j.on_left);
            let (r_table, r_col): (String, String) = helpers::split_qualified(&j.on_right);
            let on_l = if l_table.is_empty() {
                helpers::quote_col(&l_col)
            } else {
                format!(
                    "{}.{}",
                    helpers::quote_col(&l_table),
                    helpers::quote_col(&l_col)
                )
            };
            let on_r = if r_table.is_empty() {
                helpers::quote_col(&r_col)
            } else {
                format!(
                    "{}.{}",
                    helpers::quote_col(&r_table),
                    helpers::quote_col(&r_col)
                )
            };
            if j.kind == JoinKind::CrossJoin {
                format!("{kind} {}{alias_sql}", helpers::quote_col(&j.table))
            } else {
                format!(
                    "{kind} {}{alias_sql} ON {on_l} = {on_r}",
                    helpers::quote_col(&j.table)
                )
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn compile_agg_cols(anns: &[AggregateExpr]) -> String {
    anns.iter()
        .map(|a| {
            let col = if a.field == "*" {
                "*".to_string()
            } else {
                helpers::qualified_col(&a.field)
            };
            let distinct = if a.distinct && a.func != AggFunc::Count {
                "DISTINCT "
            } else if a.distinct {
                "DISTINCT "
            } else {
                ""
            };
            match &a.func {
                AggFunc::Raw(expr) => format!("{expr} AS {}", helpers::quote_col(&a.alias)),
                f => format!(
                    "{}({}{}) AS {}",
                    f.sql_name(),
                    distinct,
                    col,
                    helpers::quote_col(&a.alias)
                ),
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn compile_order_by(clauses: &[crate::ast::OrderByClause]) -> String {
    clauses
        .iter()
        .map(|c| {
            let dir = match c.direction {
                SortDirection::Asc => "ASC",
                SortDirection::Desc => "DESC",
            };
            format!("{} {dir}", helpers::qualified_col(&c.field))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn compile_where_combined(
    filters: &[FilterNode],
    q: Option<&QNode>,
    values: &mut Vec<SqlValue>,
    backend: Backend,
) -> QueryResult<String> {
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

pub fn compile_q(q: &QNode, values: &mut Vec<SqlValue>, backend: Backend) -> QueryResult<String> {
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
                .collect::<QueryResult<_>>()?;
            Ok(format!("({})", parts.join(" AND ")))
        }
        QNode::Or(children) => {
            let parts: Vec<String> = children
                .iter()
                .map(|c| compile_q(c, values, backend))
                .collect::<QueryResult<_>>()?;
            Ok(format!("({})", parts.join(" OR ")))
        }
        QNode::Not(child) => {
            let inner = compile_q(child, values, backend)?;
            Ok(format!("NOT ({inner})"))
        }
    }
}

fn compile_filters(
    filters: &[FilterNode],
    values: &mut Vec<SqlValue>,
    backend: Backend,
) -> QueryResult<String> {
    let parts: Vec<String> = filters
        .iter()
        .map(|f| compile_single_filter(&f.field, &f.lookup, &f.value, f.negated, values, backend))
        .collect::<QueryResult<_>>()?;
    Ok(parts.join(" AND "))
}

fn compile_single_filter(
    field: &str,
    lookup: &str,
    value: &SqlValue,
    negated: bool,
    values: &mut Vec<SqlValue>,
    backend: Backend,
) -> QueryResult<String> {
    let (base_column, applied_transforms, json_key) = if field.contains("__") {
        let parts: Vec<&str> = field.split("__").collect();

        let mut transforms = Vec::new();
        let mut key_part: Option<&str> = None;

        for part in parts[1..].iter() {
            if KNOWN_TRANSFORMS.contains(part) {
                transforms.push(*part);
            } else {
                key_part = Some(*part);
                break;
            }
        }

        if let Some(key) = key_part {
            (parts[0].to_string(), transforms, Some(key.to_string()))
        } else if !transforms.is_empty() {
            (parts[0].to_string(), transforms, None)
        } else {
            (field.to_string(), vec![], None)
        }
    } else {
        (field.to_string(), vec![], None)
    };

    let final_column = if lookup.contains("__") {
        helpers::qualified_col(&base_column)
    } else if !applied_transforms.is_empty() {
        let mut result = helpers::qualified_col(&base_column);
        for transform in &applied_transforms {
            result = lookups::apply_transform(transform, &result, backend, None)?;
        }
        result
    } else {
        helpers::qualified_col(&base_column)
    };

    let ctx = LookupContext {
        column: final_column.clone(),
        negated,
        backend,
        json_key: json_key.clone(),
    };

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

    if lookup == "has_any" || lookup == "has_all" {
        let items = match value {
            SqlValue::List(v) => v.clone(),
            other => vec![other.clone()],
        };
        if items.is_empty() {
            return Ok("(1 = 0)".into());
        }

        let fragment = if backend == Backend::PostgreSQL {
            let op = if lookup == "has_any" { "?|" } else { "?&" };
            format!("{final_column} {op} ?")
        } else if backend == Backend::MySQL {
            let op = if lookup == "has_any" {
                "'one'"
            } else {
                "'all'"
            };
            let ph = std::iter::repeat_n("CONCAT('$.', ?)", items.len())
                .collect::<Vec<_>>()
                .join(", ");
            format!("JSON_CONTAINS_PATH({}, {op}, {ph})", final_column)
        } else {
            // SQLite: manual expansion
            let op = if lookup == "has_any" { " OR " } else { " AND " };
            let ph = std::iter::repeat_n(
                format!("json_extract({}, '$.' || ?) IS NOT NULL", final_column),
                items.len(),
            )
            .collect::<Vec<_>>()
            .join(op);
            ph
        };
        values.extend(items);
        return Ok(if negated {
            format!("NOT ({fragment})")
        } else {
            fragment
        });
    }

    if lookup == "range" {
        let (lo, hi) = match value {
            SqlValue::List(v) if v.len() == 2 => (v[0].clone(), v[1].clone()),
            _ => return Err(QueryError::Internal("range needs exactly 2 values".into())),
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

    if lookup.contains("__") || json_key.is_some() {
        let fragment = lookups::resolve(&base_column, lookup, &ctx)?;
        values.push(value.clone());
        return Ok(if negated {
            format!("NOT ({fragment})")
        } else {
            fragment
        });
    }

    if KNOWN_TRANSFORMS.contains(&lookup) {
        let transform_fn = match lookup {
            "date" => date::date_transform as crate::lookups::LookupFn,
            "year" => date::year_transform as crate::lookups::LookupFn,
            "month" => date::month_transform as crate::lookups::LookupFn,
            "day" => date::day_transform as crate::lookups::LookupFn,
            "hour" => date::hour_transform as crate::lookups::LookupFn,
            "minute" => date::minute_transform as crate::lookups::LookupFn,
            "second" => date::second_transform as crate::lookups::LookupFn,
            "week" => date::week_transform as crate::lookups::LookupFn,
            "dow" => date::dow_transform as crate::lookups::LookupFn,
            "quarter" => date::quarter_transform as crate::lookups::LookupFn,
            "time" => date::time_transform as crate::lookups::LookupFn,
            "iso_week" => date::iso_week_transform as crate::lookups::LookupFn,
            "iso_dow" => date::iso_dow_transform as crate::lookups::LookupFn,
            "key" => json::json_key_transform as crate::lookups::LookupFn,
            "key_text" => json::json_key_text_transform as crate::lookups::LookupFn,
            "json" => json::json_cast_transform as crate::lookups::LookupFn,

            _ => {
                return Err(QueryError::UnknownLookup {
                    field: field.to_string(),
                    lookup: lookup.to_string(),
                })
            }
        };
        values.push(value.clone());
        return Ok(transform_fn(&ctx));
    }

    let fragment = lookups::resolve(&base_column, lookup, &ctx)?;
    let bound = apply_like_wrapping(lookup, value.clone());
    values.push(bound);
    Ok(if negated {
        format!("NOT ({fragment})")
    } else {
        fragment
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::*;

    #[test]
    fn test_bare_select() {
        init_registry();
        let q = compile(&QueryNode::select("posts")).unwrap();
        assert_eq!(q.sql, r#"SELECT * FROM "posts""#);
    }

    #[test]
    fn test_q_or() {
        init_registry();
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
        init_registry();
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
        init_registry();
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
        init_registry();
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
        init_registry();
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

    fn init_registry() {
        crate::lookups::init_registry();
    }
}
