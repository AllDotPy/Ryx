//
//
pub mod mysql;
pub mod postgres;
pub mod sqlite;

use ryx_common::errors::{RyxError, RyxResult};
use ryx_query::{
    ast::{QueryNode, SqlValue},
    compiler::CompiledQuery,
};
use sqlx::{Executor, MySqlConnection, PgConnection, SqliteConnection, Transaction};

use crate::pool::{PoolStats, RyxPool};
use crate::utils::decode_rows;

/// Unified connection enum to avoid dynamic dispatch in the hot path.
#[derive(Debug)]
pub enum RyxConnection {
    Postgres(PgConnection),
    MySql(MySqlConnection),
    Sqlite(SqliteConnection),
}

/// Unified transaction enum.
/// Uses 'static because transactions are held across PyO3 boundaries in Arc<Mutex<Option<...>>>.
#[derive(Debug)]
pub enum RyxTransaction {
    Postgres(Transaction<'static, sqlx::Postgres>),
    MySql(Transaction<'static, sqlx::MySql>),
    Sqlite(Transaction<'static, sqlx::Sqlite>),
}

impl RyxTransaction {
    pub async fn execute_raw(&mut self, sql: &str) -> RyxResult<()> {
        match self {
            RyxTransaction::Postgres(tx) => tx
                .execute(sqlx::query::<sqlx::Postgres>(sql))
                .await
                .map_err(RyxError::Database)
                .map(|_| ()),
            RyxTransaction::MySql(tx) => tx
                .execute(sqlx::query::<sqlx::MySql>(sql))
                .await
                .map_err(RyxError::Database)
                .map(|_| ()),
            RyxTransaction::Sqlite(tx) => tx
                .execute(sqlx::query::<sqlx::Sqlite>(sql))
                .await
                .map_err(RyxError::Database)
                .map(|_| ()),
        }
    }

    pub async fn fetch_raw(&mut self, sql: &str) -> RyxResult<Vec<DecodedRow>> {
        match self {
            RyxTransaction::Postgres(tx) => {
                let rows = tx
                    .fetch_all(sqlx::query::<sqlx::Postgres>(sql))
                    .await
                    .map_err(RyxError::Database)?;
                Ok(decode_rows(&rows, None))
            }
            RyxTransaction::MySql(tx) => {
                let rows = tx
                    .fetch_all(sqlx::query::<sqlx::MySql>(sql))
                    .await
                    .map_err(RyxError::Database)?;
                Ok(decode_rows(&rows, None))
            }
            RyxTransaction::Sqlite(tx) => {
                let rows = tx
                    .fetch_all(sqlx::query::<sqlx::Sqlite>(sql))
                    .await
                    .map_err(RyxError::Database)?;
                Ok(decode_rows(&rows, None))
            }
        }
    }

    pub async fn execute_query(&mut self, query: CompiledQuery) -> RyxResult<u64> {
        match self {
            RyxTransaction::Postgres(tx) => {
                let mut q = sqlx::query(&query.sql);
                for v in &query.values {
                    q = bind_pg(q, v);
                }
                Ok(tx
                    .execute(q)
                    .await
                    .map_err(RyxError::Database)?
                    .rows_affected())
            }
            RyxTransaction::MySql(tx) => {
                let mut q = sqlx::query(&query.sql);
                for v in &query.values {
                    q = bind_mysql(q, v);
                }
                Ok(tx
                    .execute(q)
                    .await
                    .map_err(RyxError::Database)?
                    .rows_affected())
            }
            RyxTransaction::Sqlite(tx) => {
                let mut q = sqlx::query(&query.sql);
                for v in &query.values {
                    q = bind_sqlite(q, v);
                }
                Ok(tx
                    .execute(q)
                    .await
                    .map_err(RyxError::Database)?
                    .rows_affected())
            }
        }
    }

    pub async fn fetch_query(&mut self, query: CompiledQuery) -> RyxResult<Vec<DecodedRow>> {
        match self {
            RyxTransaction::Postgres(tx) => {
                let mut q = sqlx::query(&query.sql);
                for v in &query.values {
                    q = bind_pg(q, v);
                }
                let rows = tx.fetch_all(q).await.map_err(RyxError::Database)?;
                Ok(decode_rows(&rows, query.base_table.as_deref()))
            }
            RyxTransaction::MySql(tx) => {
                let mut q = sqlx::query(&query.sql);
                for v in &query.values {
                    q = bind_mysql(q, v);
                }
                let rows = tx.fetch_all(q).await.map_err(RyxError::Database)?;
                Ok(decode_rows(&rows, query.base_table.as_deref()))
            }
            RyxTransaction::Sqlite(tx) => {
                let mut q = sqlx::query(&query.sql);
                for v in &query.values {
                    q = bind_sqlite(q, v);
                }
                let rows = tx.fetch_all(q).await.map_err(RyxError::Database)?;
                Ok(decode_rows(&rows, query.base_table.as_deref()))
            }
        }
    }
}

// Binding helpers
fn bind_pg<'q>(
    q: sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments>,
    v: &'q SqlValue,
) -> sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments> {
    match v {
        SqlValue::Null => q.bind(None::<String>),
        SqlValue::Bool(b) => q.bind(*b),
        SqlValue::Int(i) => q.bind(*i),
        SqlValue::Float(f) => q.bind(*f),
        SqlValue::Text(s)
        | SqlValue::Date(s)
        | SqlValue::DateTime(s)
        | SqlValue::Time(s)
        | SqlValue::Uuid(s)
        | SqlValue::Decimal(s)
        | SqlValue::Json(s) => q.bind(s.as_str()),
        SqlValue::List(_) => q,
    }
}

fn bind_mysql<'q>(
    q: sqlx::query::Query<'q, sqlx::MySql, sqlx::mysql::MySqlArguments>,
    v: &'q SqlValue,
) -> sqlx::query::Query<'q, sqlx::MySql, sqlx::mysql::MySqlArguments> {
    match v {
        SqlValue::Null => q.bind(None::<String>),
        SqlValue::Bool(b) => q.bind(*b),
        SqlValue::Int(i) => q.bind(*i),
        SqlValue::Float(f) => q.bind(*f),
        SqlValue::Text(s)
        | SqlValue::Date(s)
        | SqlValue::DateTime(s)
        | SqlValue::Time(s)
        | SqlValue::Uuid(s)
        | SqlValue::Decimal(s)
        | SqlValue::Json(s) => q.bind(s.as_str()),
        SqlValue::List(_) => q,
    }
}

fn bind_sqlite<'q>(
    q: sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>>,
    v: &'q SqlValue,
) -> sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>> {
    match v {
        SqlValue::Null => q.bind(None::<String>),
        SqlValue::Bool(b) => q.bind(*b),
        SqlValue::Int(i) => q.bind(*i),
        SqlValue::Float(f) => q.bind(*f),
        SqlValue::Text(s)
        | SqlValue::Date(s)
        | SqlValue::DateTime(s)
        | SqlValue::Time(s)
        | SqlValue::Uuid(s)
        | SqlValue::Decimal(s)
        | SqlValue::Json(s) => q.bind(s.as_str()),
        SqlValue::List(_) => q,
    }
}

#[async_trait::async_trait]
pub trait RyxBackend: Send + Sync + 'static {
    async fn __fetch_all(&self, query: CompiledQuery) -> RyxResult<Vec<DecodedRow>>;
    async fn __fetch_one(&self, query: CompiledQuery) -> RyxResult<DecodedRow>;
    async fn fetch_all(&self, query: CompiledQuery) -> RyxResult<Vec<DecodedRow>>;
    async fn fetch_raw(&self, sql: String, db_alias: Option<String>) -> RyxResult<Vec<DecodedRow>>;
    async fn fetch_all_compiled(&self, node: QueryNode) -> RyxResult<Vec<DecodedRow>>;
    async fn fetch_count(&self, query: CompiledQuery) -> RyxResult<i64>;
    async fn fetch_count_compiled(&self, node: QueryNode) -> RyxResult<i64>;
    async fn fetch_one(&self, query: CompiledQuery) -> RyxResult<DecodedRow>;
    async fn fetch_one_compiled(&self, node: QueryNode) -> RyxResult<DecodedRow>;
    async fn execute(&self, query: CompiledQuery) -> RyxResult<MutationResult>;
    async fn execute_compiled(&self, node: QueryNode) -> RyxResult<MutationResult>;
    async fn bulk_insert(
        &self,
        table: String,
        columns: Vec<String>,
        rows: Vec<Vec<SqlValue>>,
        returning_id: bool,
        ignore_conflicts: bool,
        db_alias: Option<String>,
    ) -> RyxResult<MutationResult>;
    async fn bulk_delete(
        &self,
        table: String,
        pk_col: String,
        pks: Vec<SqlValue>,
        db_alias: Option<String>,
    ) -> RyxResult<MutationResult>;
    async fn bulk_update(
        &self,
        table: String,
        pk_col: String,
        col_names: Vec<String>,
        field_values: Vec<Vec<SqlValue>>,
        pks: Vec<SqlValue>,
        db_alias: Option<String>,
    ) -> RyxResult<MutationResult>;
    async fn execute_raw(&self, sql: String, db_alias: Option<String>) -> RyxResult<()>;
    fn pool_stats(&self) -> PoolStats;
    fn get_pool(&self) -> RyxPool;
}

use std::sync::Arc;

/// Mapping of column names to their indices in a row.
/// Shared across all rows in a result set.
#[derive(Debug, Clone)]
pub struct RowMapping {
    pub columns: Vec<String>,
}

/// A lightweight view of a database row.
/// Instead of a HashMap, it stores values in a Vec.
#[derive(Debug, Clone)]
pub struct RowView {
    pub values: Vec<ryx_query::ast::SqlValue>,
    pub mapping: Arc<RowMapping>,
}

impl RowView {
    pub fn get(&self, name: &str) -> Option<&ryx_query::ast::SqlValue> {
        self.mapping
            .columns
            .iter()
            .position(|c| c == name)
            .and_then(|idx| self.values.get(idx))
    }
}

pub type DecodedRow = RowView;

/// Result of a non-SELECT query (INSERT/UPDATE/DELETE).
#[derive(Debug)]
pub struct MutationResult {
    pub rows_affected: u64,
    pub last_insert_id: Option<i64>,
    pub returned_ids: Option<Vec<i64>>,
}
