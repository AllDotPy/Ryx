// use crate::pool;
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyList, PyTuple};

use ryx_backend::pool as ryx_pool;
use ryx_backend::query::{
    AggFunc, AggregateExpr, FilterNode, JoinClause, JoinKind, OrderByClause, QueryNode,
    QueryOperation, Symbol,
};

use std::sync::Arc;

use crate::py_dict_to_qnode;
use crate::py_to_sql_value;

/// Build a QueryBuilder/QueryNode in one FFI call from a list of ops.
///
/// ops is a Python list of tuples: (tag, payload)
/// Supported tags:
///   - "filters": list[(field, lookup, value, negated)]
///   - "q_node": dict-repr of Q
///   - "annotations": list[(alias, func, field, distinct)]
///   - "group_by": list[str]
///   - "join": (kind, table, alias, on_left, on_right)
///   - "order_by": list[str]
///   - "limit": int
///   - "offset": int
///   - "distinct": bool
///   - "using": str
#[pyfunction]
#[pyo3(signature = (table, ops, alias=None))]
pub fn build_plan<'py>(
    table: String,
    ops: Vec<Bound<'_, PyAny>>,
    alias: Option<String>,
) -> PyResult<crate::PyQueryBuilder> {
    let backend = ryx_pool::get_backend(alias.as_deref()).map_err(crate::err_to_py)?;
    let mut node = QueryNode::select(table).with_backend(backend);
    if let Some(a) = alias {
        node = node.with_db_alias(a);
    }

    for op in ops {
        let tuple = op.cast::<PyTuple>().map_err(|_| {
            pyo3::exceptions::PyValueError::new_err("ops must be sequence of tuples")
        })?;
        if tuple.len() < 1 {
            continue;
        }
        let tag: String = tuple.get_item(0)?.extract()?;
        match tag.as_str() {
            "filters" => {
                let payload = tuple.get_item(1)?;
                let list = payload.cast::<PyList>()?;
                for item in list {
                    let t = item.cast::<PyTuple>()?;
                    let field: String = t.get_item(0)?.extract()?;
                    let lookup: String = t.get_item(1)?.extract()?;
                    let val = t.get_item(2)?;
                    let negated: bool = t.get_item(3)?.extract()?;
                    let sql_value = py_to_sql_value(&val)?;
                    node = node.with_filter(FilterNode {
                        field: field.into(),
                        lookup,
                        value: sql_value,
                        negated,
                    });
                }
            }
            "q_node" => {
                let payload = tuple.get_item(1)?;
                let q = py_dict_to_qnode(&payload)?;
                node = node.with_q(q);
            }
            "annotations" => {
                let payload = tuple.get_item(1)?;
                let list = payload.cast::<PyList>()?;
                for item in list {
                    let t = item.cast::<PyTuple>()?;
                    let alias: String = t.get_item(0)?.extract()?;
                    let func: String = t.get_item(1)?.extract()?;
                    let field: String = t.get_item(2)?.extract()?;
                    let distinct: bool = t.get_item(3)?.extract()?;
                    let agg_func = match func.as_str() {
                        "Count" => AggFunc::Count,
                        "Sum" => AggFunc::Sum,
                        "Avg" => AggFunc::Avg,
                        "Min" => AggFunc::Min,
                        "Max" => AggFunc::Max,
                        other => AggFunc::Raw(other.to_string()),
                    };
                    node = node.with_annotation(AggregateExpr {
                        alias: alias.into(),
                        func: agg_func,
                        field: field.into(),
                        distinct,
                    });
                }
            }
            "group_by" => {
                let payload = tuple.get_item(1)?;
                let list = payload.cast::<PyList>()?;
                for item in list {
                    let field: String = item.extract()?;
                    node = node.with_group_by(field);
                }
            }
            "select_cols" => {
                let payload = tuple.get_item(1)?;
                let list = payload.cast::<PyList>()?;
                let cols: Vec<Symbol> = list
                    .iter()
                    .map(|i| i.extract::<String>().unwrap_or_default().into())
                    .collect();
                node.operation = QueryOperation::Select {
                    columns: Some(cols),
                };
            }
            "join" => {
                let payload = tuple.get_item(1)?;
                let t = payload.cast::<PyTuple>()?;
                let kind: String = t.get_item(0)?.extract()?;
                let table: String = t.get_item(1)?.extract()?;
                let alias_opt: String = t.get_item(2)?.extract()?;
                let on_left: String = t.get_item(3)?.extract()?;
                let on_right: String = t.get_item(4)?.extract()?;
                let join_kind = match kind.as_str() {
                    "LEFT" | "LEFT OUTER" => JoinKind::LeftOuter,
                    "RIGHT" | "RIGHT OUTER" => JoinKind::RightOuter,
                    "FULL" | "FULL OUTER" => JoinKind::FullOuter,
                    "CROSS" => JoinKind::CrossJoin,
                    _ => JoinKind::Inner,
                };
                let alias = if alias_opt.is_empty() {
                    None
                } else {
                    Some(alias_opt.into())
                };
                node = node.with_join(JoinClause {
                    kind: join_kind,
                    table: table.into(),
                    alias,
                    on_left,
                    on_right,
                });
            }
            "order_by" => {
                let payload = tuple.get_item(1)?;
                let list = payload.cast::<PyList>()?;
                for item in list {
                    let field: String = item.extract()?;
                    node = node.with_order_by(OrderByClause::parse(&field));
                }
            }
            "limit" => {
                let n: u64 = tuple.get_item(1)?.extract()?;
                node = node.with_limit(n);
            }
            "offset" => {
                let n: u64 = tuple.get_item(1)?.extract()?;
                node = node.with_offset(n);
            }
            "distinct" => {
                let flag: bool = tuple.get_item(1)?.extract()?;
                if flag {
                    let mut n = node.clone();
                    n.distinct = true;
                    node = n;
                }
            }
            "using" => {
                let db_alias: String = tuple.get_item(1)?.extract()?;
                let backend = ryx_pool::get_backend(Some(&db_alias)).map_err(crate::err_to_py)?;
                node = node.with_backend(backend).with_db_alias(db_alias);
            }
            _ => {}
        }
    }

    Ok(crate::PyQueryBuilder {
        node: Arc::new(node),
    })
}
