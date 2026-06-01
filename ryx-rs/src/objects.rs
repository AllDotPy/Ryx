use std::marker::PhantomData;

use ryx_common::{RyxResult, SqlValue};
use ryx_query::ast::{QueryNode, QueryOperation};
use ryx_query::symbols::Symbol;

use crate::into_sql::IntoSqlValue;
use crate::model::Model;
use crate::queryset::QuerySet;
use crate::row::FromRow;

/// Entry point for query operations on a model.
///
/// Usage:
/// ```ignore
/// User::objects().filter("age__gte", 18).all().await?;
/// User::objects().get("email", "john@doe.com").await?;
/// User::objects().create().set("name", "John").save().await?;
/// ```
pub struct ObjectsManager<T> {
    _marker: PhantomData<T>,
}

impl<T> ObjectsManager<T> {
    pub fn new() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}

impl<T: Model + FromRow> ObjectsManager<T> {
    pub fn all(self) -> QuerySet<T> {
        QuerySet::new(T::table_name())
    }

    pub fn filter(self, field: &str, value: impl IntoSqlValue) -> QuerySet<T> {
        QuerySet::new(T::table_name()).filter((field, value))
    }

    pub fn exclude(self, field: &str, value: impl IntoSqlValue) -> QuerySet<T> {
        QuerySet::new(T::table_name()).exclude((field, value))
    }

    pub fn get(self, field: &str, value: impl IntoSqlValue) -> QuerySet<T> {
        QuerySet::new(T::table_name()).filter((field, value))
    }

    pub fn create(self) -> InsertBuilder<T> {
        InsertBuilder::new()
    }
}

// === INSERT BUILDER ===

pub struct InsertBuilder<T> {
    values: Vec<(String, SqlValue)>,
    _marker: PhantomData<T>,
}

impl<T: Model + FromRow> InsertBuilder<T> {
    pub fn new() -> Self {
        Self {
            values: Vec::new(),
            _marker: PhantomData,
        }
    }

    pub fn set(mut self, field: &str, value: impl IntoSqlValue) -> Self {
        self.values
            .push((field.to_string(), value.into_sql_value()));
        self
    }

    pub async fn save(self) -> RyxResult<T> {
        let table = T::table_name();
        let backend = ryx_backend::pool::get_backend(None)
            .unwrap_or(ryx_query::Backend::PostgreSQL);
        let mut node = QueryNode::select(table);
        node.backend = backend;
        node.operation = QueryOperation::Insert {
            values: self
                .values
                .into_iter()
                .map(|(k, v)| (Symbol::from(k.as_str()), v))
                .collect(),
            returning_id: true,
        };
        let b = ryx_backend::pool::get(node.db_alias.as_deref())?;
        let compiled = ryx_query::compiler::compile(&node)?;
        let mut rows = b.fetch_all(compiled).await?;
        match rows.is_empty() {
            true => Err(ryx_common::RyxError::Internal(
                "Insert returned no rows".into(),
            )),
            false => {
                let row = rows.remove(0);
                T::from_row(&row)
            }
        }
    }
}
