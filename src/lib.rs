use std::collections::HashMap;
use std::sync::Arc;

use pyo3::prelude::IntoPyObject;
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyFloat, PyInt, PyList, PyString, PyTuple};
use tokio::sync::Mutex as TokioMutex;

pub mod errors;
pub mod executor;
pub mod pool;
pub mod transaction;

use crate::errors::RyxError;
use crate::pool::PoolConfig;
use ryx_query::ast::{
    AggFunc, AggregateExpr, FilterNode, JoinClause, JoinKind, OrderByClause, QNode, QueryNode,
    QueryOperation, SqlValue,
};
use ryx_query::compiler;
use ryx_query::lookups;
use crate::transaction::TransactionHandle;

// ###
// Setup / pool functions
// ###

#[pyfunction]
#[pyo3(signature = (
    urls,
    max_connections = 10,
    min_connections = 1,
    connect_timeout = 30,
    idle_timeout = 600,
    max_lifetime = 1800,
))]
fn setup<'py>(
    py: Python<'py>,
    urls: Bound<'_, PyAny>,
    max_connections: u32,
    min_connections: u32,
    connect_timeout: u64,
    idle_timeout: u64,
    max_lifetime: u64,
) -> PyResult<Bound<'py, PyAny>> {
    let urls_py = urls.cast::<PyDict>()?;
    let mut database_urls = HashMap::new();
 
    for (key, value) in urls_py.iter() {
        let alias = key.cast::<PyString>()?.to_str()?.to_string();
        let url = value.cast::<PyString>()?.to_str()?.to_string();
        database_urls.insert(alias, url);
    }
 
    let config = PoolConfig {
        max_connections,
        min_connections,
        connect_timeout_secs: connect_timeout,
        idle_timeout_secs: idle_timeout,
        max_lifetime_secs: max_lifetime,
    };
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        pool::initialize(database_urls, config).await.map_err(PyErr::from)?;
        Python::attach(|py| Ok(py.None().into_pyobject(py)?.unbind()))
    })
}

#[pyfunction]
fn register_lookup(name: String, sql_template: String) -> PyResult<()> {
    lookups::register_custom(name, sql_template).map_err(RyxError::from).map_err(PyErr::from)
}

#[pyfunction]
fn available_lookups() -> PyResult<Vec<String>> {
    lookups::registered_lookups().map_err(RyxError::from).map_err(PyErr::from)
}

#[pyfunction]
fn list_lookups<'py>() -> Vec<&'static str> {
    lookups::all_lookups().to_vec()
}

#[pyfunction]
fn list_transforms() -> Vec<&'static str> {
    lookups::all_transforms().to_vec()
}


#[pyfunction]
fn is_connected(_py: Python<'_>, alias: Option<String>) -> bool {
    // For now we just check if the registry is initialized
    pool::is_initialized(alias)
}

#[pyfunction]
fn pool_stats<'py>(py: Python<'py>, alias: Option<String>) -> PyResult<Bound<'py, PyAny>> {
    let stats = pool::stats(alias.as_deref()).map_err(PyErr::from)?;
    let dict = PyDict::new(py);
    dict.set_item("size", stats.size)?;
    dict.set_item("idle", stats.idle)?;
    Ok(dict.into_any())
}

#[pyfunction]
#[pyo3(signature = (sql, alias=None))]
fn raw_fetch<'py>(
    py: Python<'py>,
    sql: String,
    alias: Option<String>,
) -> PyResult<Bound<'py, PyAny>> {
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let rows = executor::fetch_raw(sql, alias).await.map_err(PyErr::from)?;
        Python::attach(|py| {
            let py_rows = decoded_rows_to_py(py, rows)?;
            Ok(py_rows.unbind())
        })
    })
}
 
#[pyfunction]
#[pyo3(signature = (sql, alias=None))]
fn raw_execute<'py>(
    py: Python<'py>,
    sql: String,
    alias: Option<String>,
) -> PyResult<Bound<'py, PyAny>> {
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        executor::execute_raw(sql, alias).await.map_err(PyErr::from)?;
        Python::attach(|py| Ok(py.None().into_pyobject(py)?.unbind()))
    })
}



// ###
// QueryBuilder
// ###

#[pyclass(from_py_object, name = "QueryBuilder")]
#[derive(Clone)]
pub struct PyQueryBuilder {
    node: Arc<QueryNode>,
}

#[pymethods]
impl PyQueryBuilder {
    #[new]
    fn new(table: String) -> PyResult<Self> {
        // Get the backend from the pool at QueryBuilder creation time
        let backend = pool::get_backend(None).unwrap_or(ryx_query::Backend::PostgreSQL);
        
        Ok(Self {
            node: Arc::new(QueryNode::select(table).with_backend(backend)),
        })
    }
 
    fn set_using(&self, alias: String) -> PyResult<PyQueryBuilder> {
        Ok(PyQueryBuilder {
            node: Arc::new(self.node.as_ref().clone().with_db_alias(alias)),
        })
    }

    fn add_filter(
        &self,
        field: String,
        lookup: String,
        value: &Bound<'_, PyAny>,
        negated: bool,
    ) -> PyResult<PyQueryBuilder> {
        let sql_value = py_to_sql_value(value)?;
        Ok(PyQueryBuilder {
            node: Arc::new(self.node.as_ref().clone().with_filter(FilterNode {
                field,
                lookup,
                value: sql_value,
                negated,
            })),
        })
    }

    /// Add multiple filters in a single FFI call to reduce overhead when applying
    /// many kwargs-based filters from Python.
    fn add_filters_batch(
        &self,
        filters: Vec<(String, String, Bound<'_, PyAny>, bool)>,
    ) -> PyResult<PyQueryBuilder> {
        let mut node = self.node.as_ref().clone();
        for (field, lookup, value, negated) in filters {
            let sql_value = py_to_sql_value(&value)?;
            node = node.with_filter(FilterNode {
                field,
                lookup,
                value: sql_value,
                negated,
            });
        }
        Ok(PyQueryBuilder { node: Arc::new(node) })
    }

    fn add_q_node(&self, node: &Bound<'_, PyAny>) -> PyResult<PyQueryBuilder> {
        let q = py_dict_to_qnode(node)?;
        Ok(PyQueryBuilder {
            node: Arc::new(self.node.as_ref().clone().with_q(q)),
        })
    }

    fn add_annotation(
        &self,
        alias: String,
        func: String,
        field: String,
        distinct: bool,
    ) -> PyQueryBuilder {
        let agg_func = match func.as_str() {
            "Count" => AggFunc::Count,
            "Sum" => AggFunc::Sum,
            "Avg" => AggFunc::Avg,
            "Min" => AggFunc::Min,
            "Max" => AggFunc::Max,
            other => AggFunc::Raw(other.to_string()),
        };
        PyQueryBuilder {
            node: Arc::new(self.node.as_ref().clone().with_annotation(AggregateExpr {
                alias,
                func: agg_func,
                field,
                distinct,
            })),
        }
    }

    fn add_group_by(&self, field: String) -> PyQueryBuilder {
        PyQueryBuilder {
            node: Arc::new(self.node.as_ref().clone().with_group_by(field)),
        }
    }

    fn add_join(
        &self,
        kind: String,
        table: String,
        alias: String,
        on_left: String,
        on_right: String,
    ) -> PyQueryBuilder {
        let join_kind = match kind.as_str() {
            "LEFT" | "LEFT OUTER" => JoinKind::LeftOuter,
            "RIGHT" | "RIGHT OUTER" => JoinKind::RightOuter,
            "FULL" | "FULL OUTER" => JoinKind::FullOuter,
            "CROSS" => JoinKind::CrossJoin,
            _ => JoinKind::Inner,
        };
        let alias_opt = if alias.is_empty() { None } else { Some(alias) };
        PyQueryBuilder {
            node: Arc::new(self.node.as_ref().clone().with_join(JoinClause {
                kind: join_kind,
                table,
                alias: alias_opt,
                on_left,
                on_right,
            })),
        }
    }

    fn add_order_by(&self, field: String) -> PyQueryBuilder {
        PyQueryBuilder {
            node: Arc::new(
                self.node
                    .as_ref()
                    .clone()
                    .with_order_by(OrderByClause::parse(&field)),
            ),
        }
    }

    /// Batch add ORDER BY clauses to reduce repeated crossings.
    fn add_order_by_batch(&self, fields: Vec<String>) -> PyQueryBuilder {
        let mut node = self.node.as_ref().clone();
        for f in fields {
            node = node.with_order_by(OrderByClause::parse(&f));
        }
        PyQueryBuilder { node: Arc::new(node) }
    }

    fn set_limit(&self, n: u64) -> PyQueryBuilder {
        PyQueryBuilder {
            node: Arc::new(self.node.as_ref().clone().with_limit(n)),
        }
    }

    fn set_offset(&self, n: u64) -> PyQueryBuilder {
        PyQueryBuilder {
            node: Arc::new(self.node.as_ref().clone().with_offset(n)),
        }
    }

    fn set_distinct(&self) -> PyQueryBuilder {
        let mut node = self.node.as_ref().clone();
        node.distinct = true;
        PyQueryBuilder { node: Arc::new(node) }
    }

    // # Execution methods

    fn fetch_all<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let node = self.node.as_ref().clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let rows = executor::fetch_all_compiled(node).await.map_err(PyErr::from)?;
            Python::attach(|py| Ok(decoded_rows_to_py(py, rows)?.unbind()))
        })
    }

    fn fetch_first<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let node = self.node.as_ref().clone().with_limit(1);
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let rows = executor::fetch_all_compiled(node).await.map_err(PyErr::from)?;
            Python::attach(|py| match rows.into_iter().next() {
                Some(row) => Ok(decoded_row_to_py(py, row)?.into_any().unbind()),
                None => Ok(py.None().into_pyobject(py)?.unbind()),
            })
        })
    }

    fn fetch_get<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let node = self.node.as_ref().clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let row = executor::fetch_one_compiled(node).await.map_err(PyErr::from)?;
            Python::attach(|py| Ok(decoded_row_to_py(py, row)?.into_any().unbind()))
        })
    }

    fn fetch_count<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let mut count_node = self.node.as_ref().clone();
        count_node.operation = QueryOperation::Count;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let count = executor::fetch_count_compiled(count_node).await.map_err(PyErr::from)?;
            Python::attach(|py| Ok(count.into_pyobject(py)?.unbind()))
        })
    }

    fn fetch_aggregate<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let mut agg_node = self.node.as_ref().clone();
        agg_node.operation = QueryOperation::Aggregate;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let rows = executor::fetch_all_compiled(agg_node).await.map_err(PyErr::from)?;
            Python::attach(|py| match rows.into_iter().next() {
                Some(row) => Ok(decoded_row_to_py(py, row)?.into_any().unbind()),
                None => Ok(PyDict::new(py).into_any().unbind()),
            })
        })
    }

    fn execute_delete<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let mut del_node = self.node.as_ref().clone();
        del_node.operation = QueryOperation::Delete;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let res = executor::execute_compiled(del_node).await.map_err(PyErr::from)?;
            Python::attach(|py| Ok(res.rows_affected.into_pyobject(py)?.unbind()))
        })
    }

    fn execute_update<'py>(
        &self,
        py: Python<'py>,
        assignments: Vec<(String, Bound<'_, PyAny>)>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let rust_assignments: Vec<(String, SqlValue)> = assignments
            .into_iter()
            .map(|(col, val)| Ok::<_, PyErr>((col, py_to_sql_value(&val)?)))
            .collect::<Result<_, _>>()?;

        let mut upd_node = self.node.as_ref().clone();
        upd_node.operation = QueryOperation::Update {
            assignments: rust_assignments,
        };

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let res = executor::execute_compiled(upd_node).await.map_err(PyErr::from)?;
            Python::attach(|py| Ok(res.rows_affected.into_pyobject(py)?.unbind()))
        })
    }

    fn execute_insert<'py>(
        &self,
        py: Python<'py>,
        values: Vec<(String, Bound<'_, PyAny>)>,
        returning_id: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let rust_values: Vec<(String, SqlValue)> = values
            .into_iter()
            .map(|(col, val)| Ok::<_, PyErr>((col, py_to_sql_value(&val)?)))
            .collect::<Result<_, _>>()?;

        let mut ins_node = self.node.as_ref().clone();
        ins_node.operation = QueryOperation::Insert {
            values: rust_values,
            returning_id,
        };

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let res = executor::execute_compiled(ins_node).await.map_err(PyErr::from)?;
            Python::attach(|py| match res.last_insert_id {
                Some(id) => Ok(id.into_pyobject(py)?.unbind()),
                None => Ok(res.rows_affected.into_pyobject(py)?.unbind()),
            })
        })
    }

    fn compiled_sql(&self) -> PyResult<String> {
        Ok(compiler::compile(&self.node).map_err(RyxError::from)?.sql)
    }
}

// ###
// Type conversion: Python → Rust
// ###

fn py_to_sql_value(obj: &Bound<'_, PyAny>) -> PyResult<SqlValue> {
    if obj.is_none() {
        return Ok(SqlValue::Null);
    }
    if let Ok(b) = obj.cast::<PyBool>() {
        return Ok(SqlValue::Bool(b.is_true()));
    }
    if let Ok(i) = obj.cast::<PyInt>() {
        return Ok(SqlValue::Int(i.extract()?));
    }
    if let Ok(f) = obj.cast::<PyFloat>() {
        return Ok(SqlValue::Float(f.extract()?));
    }
    if let Ok(s) = obj.cast::<PyString>() {
        return Ok(SqlValue::Text(s.to_str()?.to_string()));
    }
    if let Ok(list) = obj.cast::<PyList>() {
        let items = list
            .iter()
            .map(|i| py_to_sql_value(&i).map(Box::new))
            .collect::<PyResult<smallvec::SmallVec<[Box<SqlValue>; 4]>>>()?;
        return Ok(SqlValue::List(items));
    }
    if let Ok(tup) = obj.cast::<PyTuple>() {
        let items = tup
            .iter()
            .map(|i| py_to_sql_value(&i).map(Box::new))
            .collect::<PyResult<smallvec::SmallVec<[Box<SqlValue>; 4]>>>()?;
        return Ok(SqlValue::List(items));
    }
    Ok(SqlValue::Text(obj.str()?.to_str()?.to_string()))
}

/// Convert a Python list of integers to a list of SqlValue::Int.
///
/// This is a fast path that skips the full type-checking cascade
/// (None → Bool → Int → Float → String → List → Tuple → str)
/// for every element. Used by bulk_delete for PK lists.
fn py_int_list_to_sql_values(list: &Bound<'_, PyList>) -> PyResult<Vec<SqlValue>> {
    list.iter()
        .map(|item| {
            let n: i64 = item.extract()?;
            Ok(SqlValue::Int(n))
        })
        .collect()
}

fn py_dict_to_qnode(obj: &Bound<'_, PyAny>) -> PyResult<QNode> {
    let dict = obj
        .cast::<PyDict>()
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("Q node must be a dict"))?;

    let node_type: String = dict
        .get_item("type")?
        .ok_or_else(|| pyo3::exceptions::PyValueError::new_err("Q node missing 'type'"))?
        .extract()?;

    match node_type.as_str() {
        "leaf" => {
            let field: String = dict
                .get_item("field")?
                .ok_or_else(|| pyo3::exceptions::PyValueError::new_err("leaf missing field"))?
                .extract()?;
            let lookup: String = dict
                .get_item("lookup")?
                .ok_or_else(|| pyo3::exceptions::PyValueError::new_err("leaf missing lookup"))?
                .extract()?;
            let negated: bool = dict
                .get_item("negated")?
                .map(|v| v.extract::<bool>().unwrap_or(false))
                .unwrap_or(false);
            let value_obj = dict
                .get_item("value")?
                .ok_or_else(|| pyo3::exceptions::PyValueError::new_err("leaf missing value"))?;
            let value = py_to_sql_value(&value_obj)?;
            Ok(QNode::Leaf {
                field,
                lookup,
                value,
                negated,
            })
        }
        "and" => Ok(QNode::And(py_dict_children(dict)?)),
        "or" => Ok(QNode::Or(py_dict_children(dict)?)),
        "not" => {
            let children = py_dict_children(dict)?;
            let first = children.into_iter().next().ok_or_else(|| {
                pyo3::exceptions::PyValueError::new_err("NOT node has no children")
            })?;
            Ok(QNode::Not(Box::new(first)))
        }
        other => Err(pyo3::exceptions::PyValueError::new_err(format!(
            "Unknown Q node type: {other}"
        ))),
    }
}

fn py_dict_children(dict: &Bound<'_, PyDict>) -> PyResult<Vec<QNode>> {
    let children_obj = dict
        .get_item("children")?
        .ok_or_else(|| pyo3::exceptions::PyValueError::new_err("Q node missing 'children'"))?;
    let children_list = children_obj
        .cast::<PyList>()
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("'children' must be a list"))?;
    children_list.iter().map(|c| py_dict_to_qnode(&c)).collect()
}

// ###
// Type conversion: Rust → Python
// ###

fn decoded_row_to_py<'py>(
    py: Python<'py>,
    row: HashMap<String, SqlValue>,
) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    for (k, v) in row {
        dict.set_item(k, sql_to_py(py, &v)?)?;
    }
    Ok(dict)
}

fn decoded_rows_to_py<'py>(
    py: Python<'py>,
    rows: Vec<HashMap<String, SqlValue>>,
) -> PyResult<Bound<'py, PyList>> {
    let list = PyList::empty(py);
    for row in rows {
        list.append(decoded_row_to_py(py, row)?)?;
    }
    Ok(list)
}

fn sql_to_py<'py>(py: Python<'py>, v: &SqlValue) -> PyResult<Py<PyAny>> {
    Ok(match v {
        SqlValue::Null => py.None(),
        SqlValue::Bool(b) => {
            let py_bool = (*b).into_pyobject(py)?;
            <pyo3::Bound<'_, PyBool> as Clone>::clone(&py_bool)
                .into_any()
                .unbind()
        }
        SqlValue::Int(i) => i.into_pyobject(py)?.into_any().unbind(),
        SqlValue::Float(f) => f.into_pyobject(py)?.into_any().unbind(),
        SqlValue::Text(s) => s.into_pyobject(py)?.into_any().unbind(),
        SqlValue::List(items) => {
            let list = PyList::empty(py);
            for item in items {
                list.append(sql_to_py(py, item)?)?;
            }
            list.into_any().unbind()
        }
    })
}

// ###
// PyTransactionHandle
// ###

#[pyclass(name = "TransactionHandle")]
pub struct PyTransactionHandle {
    pub handle: Arc<TokioMutex<Option<TransactionHandle>>>,
}

#[pymethods]
impl PyTransactionHandle {
    fn get_alias(&self) -> PyResult<Option<String>> {
        let h = self.handle.blocking_lock();
        if let Some(tx) = h.as_ref() {
            Ok(tx.alias.clone())
        } else {
            Ok(None)
        }
    }

    fn commit<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let h = self.handle.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let g = h.lock().await;
            if let Some(tx) = g.as_ref() {
                tx.commit().await.map_err(PyErr::from)?;
            }
            Python::attach(|py| Ok(py.None().into_pyobject(py)?.unbind()))
        })
    }

    fn rollback<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let h = self.handle.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let g = h.lock().await;
            if let Some(tx) = g.as_ref() {
                tx.rollback().await.map_err(PyErr::from)?;
            }
            Python::attach(|py| Ok(py.None().into_pyobject(py)?.unbind()))
        })
    }

    fn savepoint<'py>(&self, py: Python<'py>, name: String) -> PyResult<Bound<'py, PyAny>> {
        let h = self.handle.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut g = h.lock().await;
            if let Some(tx) = g.as_mut() {
                tx.savepoint(&name).await.map_err(PyErr::from)?;
            }
            Python::attach(|py| Ok(py.None().into_pyobject(py)?.unbind()))
        })
    }

    fn rollback_to<'py>(&self, py: Python<'py>, name: String) -> PyResult<Bound<'py, PyAny>> {
        let h = self.handle.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let g = h.lock().await;
            if let Some(tx) = g.as_ref() {
                tx.rollback_to(&name).await.map_err(PyErr::from)?;
            }
            Python::attach(|py| Ok(py.None().into_pyobject(py)?.unbind()))
        })
    }

    fn is_active<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let h = self.handle.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let g = h.lock().await;
            let active = if let Some(tx) = g.as_ref() {
                tx.is_active().await
            } else {
                false
            };
            Python::attach(|py| {
                let py_bool = active.into_pyobject(py)?;
                Ok(<pyo3::Bound<'_, PyBool> as Clone>::clone(&py_bool)
                    .into_any()
                    .unbind())
            })
        })
    }
}

#[pyfunction]
fn begin_transaction<'py>(
    py: Python<'py>,
    alias: Option<Bound<'_, PyString>>,
) -> PyResult<Bound<'py, PyAny>> {
    let alias_str = alias.map(|s| s.to_string());
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let handle = TransactionHandle::begin(alias_str).await.map_err(PyErr::from)?;
        Python::attach(|py| {
            let py_handle = PyTransactionHandle {
                handle: Arc::new(TokioMutex::new(Some(handle))),
            };
            Ok(Py::new(py, py_handle)?.into_any())
        })
    })
}

#[pyfunction]
fn _set_active_transaction(tx: Option<Bound<'_, PyTransactionHandle>>) -> PyResult<()> {
    if let Some(tx_ref) = tx {
        transaction::set_current_transaction(Some(tx_ref.borrow().handle.clone()));
    } else {
        transaction::set_current_transaction(None);
    }
    Ok(())
}

#[pyfunction]
fn _get_active_transaction(py: Python<'_>) -> PyResult<Option<Py<PyTransactionHandle>>> {
    if let Some(tx_arc) = transaction::get_current_transaction() {
        let py_handle = PyTransactionHandle { handle: tx_arc };
        Ok(Some(Py::new(py, py_handle)?))
    } else {
        Ok(None)
    }
}

// ###
// Raw Parameterized SQL
// ###

#[pyfunction]
fn execute_with_params<'py>(
    py: Python<'py>,
    sql: String,
    values: Vec<Bound<'_, PyAny>>,
) -> PyResult<Bound<'py, PyAny>> {
    let sql_values: Vec<SqlValue> = values
        .iter()
        .map(py_to_sql_value)
        .collect::<PyResult<_>>()?;
 
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let compiled = compiler::CompiledQuery {
            sql,
            values: sql_values.into(),
            db_alias: None,
        };
        let result = executor::execute(compiled).await.map_err(PyErr::from)?;
        Python::attach(|py| Ok(result.rows_affected.into_pyobject(py)?.unbind()))
    })
}

 
#[pyfunction]
fn fetch_with_params<'py>(
    py: Python<'py>,
    sql: String,
    values: Vec<Bound<'_, PyAny>>,
) -> PyResult<Bound<'py, PyAny>> {
    let sql_values: Vec<SqlValue> = values
        .iter()
        .map(py_to_sql_value)
        .collect::<PyResult<_>>()?;
 
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let compiled = compiler::CompiledQuery {
            sql,
            values: sql_values.into(),
            db_alias: None,
        };
        let rows = executor::fetch_all(compiled).await.map_err(PyErr::from)?;
        Python::attach(|py| Ok(decoded_rows_to_py(py, rows)?.unbind()))
    })
}



/// Bulk delete by primary key list in a single FFI call.
///
/// Equivalent to:
///   builder = QueryBuilder(table)
///   builder = builder.add_filter(pk_col, "in", pks, False)
///   await builder.execute_delete()
///
/// But avoids 3 separate FFI crossings and intermediate allocations.
#[pyfunction]
fn bulk_delete<'py>(
    py: Python<'py>,
    table: String,
    pk_col: String,
    pks: Vec<Bound<'_, PyAny>>,
) -> PyResult<Bound<'py, PyAny>> {
    let pk_list = PyList::new(py, pks)?;
    let pk_values = py_int_list_to_sql_values(&pk_list)?;

    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let result = executor::bulk_delete(table, pk_col, pk_values, None)
            .await
            .map_err(PyErr::from)?;
        Python::attach(|py| {
            let n = (result.rows_affected as i64).into_pyobject(py)?;
            Ok(n.unbind())
        })
    })
}

/// Bulk update using CASE WHEN in a single FFI call.
///
/// Builds a single UPDATE statement with CASE WHEN clauses:
///   UPDATE "table" SET
///       "col1" = CASE "pk" WHEN 1 THEN ? WHEN 2 THEN ? END,
///       "col2" = CASE "pk" WHEN 1 THEN ? WHEN 2 THEN ? END
///   WHERE "pk" IN (?, ?, ...)
///
/// All values are passed as a flat list: [pk1, val1, pk2, val2, ..., pk1, pk2, ...]
/// where the first N*F values are the CASE WHEN pairs (N rows × F fields)
/// and the last N values are the WHERE IN clause.
#[pyfunction]
fn bulk_update<'py>(
    py: Python<'py>,
    table: String,
    pk_col: String,
    // List of (column_name, list_of_values) tuples
    // Each list_of_values has the same length as pks
    columns: Vec<(String, Vec<Bound<'_, PyAny>>)>,
    pks: Vec<Bound<'_, PyAny>>,
) -> PyResult<Bound<'py, PyAny>> {
    // Convert PKs to integers (fast path)
    let pk_list = PyList::new(py, pks.clone())?;
    let pk_values = py_int_list_to_sql_values(&pk_list)?;

    // Convert all field values
    let mut field_values: Vec<Vec<SqlValue>> = Vec::with_capacity(columns.len());
    let mut col_names: Vec<String> = Vec::with_capacity(columns.len());
    for (col_name, vals) in columns {
        let sql_vals: Vec<SqlValue> = vals
            .iter()
            .map(|v| py_to_sql_value(v))
            .collect::<PyResult<_>>()?;
        field_values.push(sql_vals);
        col_names.push(col_name);
    }

    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let n = pk_values.len();
        let f = field_values.len();

        // Build CASE WHEN clauses
        let mut case_clauses = Vec::with_capacity(f);
        let mut all_values = Vec::with_capacity(n * f * 2 + n);

        for (fi, col_name) in col_names.iter().enumerate() {
            let mut case_parts = Vec::with_capacity(n * 3 + 2);
            case_parts.push(format!("\"{}\" = CASE \"{}\"", col_name, pk_col));
            for i in 0..n {
                case_parts.push("WHEN ? THEN ?".to_string());
                all_values.push(pk_values[i].clone());
                all_values.push(field_values[fi][i].clone());
            }
            case_parts.push("END".to_string());
            case_clauses.push(case_parts.join(" "));
        }

        // WHERE IN clause
        let pk_placeholders: Vec<String> = (0..n).map(|_| "?".to_string()).collect();
        for pk in &pk_values {
            all_values.push(pk.clone());
        }

        let sql = format!(
            "UPDATE \"{}\" SET {} WHERE \"{}\" IN ({})",
            table,
            case_clauses.join(", "),
            pk_col,
            pk_placeholders.join(", ")
        );

        let compiled = compiler::CompiledQuery {
            sql,
            values: all_values.into(),
            db_alias: None,
        };
        let result = executor::execute(compiled).await.map_err(PyErr::from)?;
        Python::attach(|py| {
            let n = (result.rows_affected as i64).into_pyobject(py)?;
            Ok(n.unbind())
        })
    })
}

// ###
// Module definition
// ###

#[pymodule]
fn ryx_core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    lookups::init_registry();

    let mut builder = tokio::runtime::Builder::new_multi_thread();
    builder.worker_threads(4).enable_all();
    pyo3_async_runtimes::tokio::init(builder);

    m.add_class::<PyQueryBuilder>()?;
    m.add_class::<PyTransactionHandle>()?;
    m.add_function(wrap_pyfunction!(begin_transaction, m)?)?;
    m.add_function(wrap_pyfunction!(_set_active_transaction, m)?)?;
    m.add_function(wrap_pyfunction!(_get_active_transaction, m)?)?;
    m.add_function(wrap_pyfunction!(setup, m)?)?;
    m.add_function(wrap_pyfunction!(register_lookup, m)?)?;
    m.add_function(wrap_pyfunction!(available_lookups, m)?)?;
    m.add_function(wrap_pyfunction!(list_lookups, m)?)?;
    m.add_function(wrap_pyfunction!(list_transforms, m)?)?;
    m.add_function(wrap_pyfunction!(is_connected, m)?)?;
    m.add_function(wrap_pyfunction!(pool_stats, m)?)?;
    m.add_function(wrap_pyfunction!(raw_fetch, m)?)?;
    m.add_function(wrap_pyfunction!(raw_execute, m)?)?;
    m.add_function(wrap_pyfunction!(execute_with_params, m)?)?;
    m.add_function(wrap_pyfunction!(fetch_with_params, m)?)?;
    m.add_function(wrap_pyfunction!(bulk_delete, m)?)?;
    m.add_function(wrap_pyfunction!(bulk_update, m)?)?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
