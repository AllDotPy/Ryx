// Sqlite Backend for Ryx Query Compiler

use smallvec::SmallVec;
use sqlx::{
    Column, Row,
    sqlite::{SqlitePool, SqlitePoolOptions},
};

use ryx_core::errors::{RyxError, RyxResult};
use ryx_query::ast::{QueryNode, SqlValue};
use ryx_query::compiler::{CompiledQuery, compile};

use super::{DecodedRow, MutationResult, RyxBackend};
use crate::pool::{PoolConfig, PoolStats, RyxPool};
use crate::transaction::get_current_transaction;
use crate::utils::{decode_row, decode_rows, is_date, is_timestamp};

use tracing::{debug, instrument};

pub struct SqliteBackend {
    // The connection pool for Sqlite
    pool: SqlitePool,
}

impl SqliteBackend {
    /// Create a new SqliteBackend with a connection pool based on the provided config.
    /// Uses `sqlx::SqlitePool` under the hood.
    /// Usage:
    /// ```
    /// let config = PoolConfig {
    ///     url: "sqlite:///path/to/database.db".to_string(),
    ///     max_connections: 10,
    ///     min_connections: 1,
    ///     connect_timeout_secs: 5,
    ///     idle_timeout_secs: 300,
    ///     max_lifetime_secs: 1800,
    /// };
    /// let backend = SqliteBackend::new(config, url).await;
    /// ```
    pub async fn new(config: PoolConfig, url: String) -> Self {
        // Create a new Sqlite connection pool using the provided config
        let pool = SqlitePoolOptions::new()
            .max_connections(config.max_connections)
            .min_connections(config.min_connections)
            .acquire_timeout(std::time::Duration::from_secs(config.connect_timeout_secs))
            .idle_timeout(std::time::Duration::from_secs(config.idle_timeout_secs))
            .max_lifetime(std::time::Duration::from_secs(config.max_lifetime_secs))
            .connect(&url)
            .await
            .expect("Failed to create Sqlite connection pool");
        Self { pool }
    }

    /// Begin a new transaction by acquiring a connection from the pool.
    /// Usage:
    /// ```
    /// let tx = backend.begin().await.unwrap();
    ///
    pub async fn begin(&self) -> RyxResult<sqlx::Transaction<'_, sqlx::Sqlite>> {
        self.pool.begin().await.map_err(RyxError::Database)
    }

    /// Bind all `SqlValue`s to a sqlx query in order.
    ///
    /// sqlx's `.bind()` takes ownership and returns a new query, so we chain
    /// calls with a mutable variable rather than a functional fold to keep the
    /// code readable.
    fn bind_values<'q>(
        &self,
        mut q: sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>>,
        values: &'q [SqlValue],
    ) -> sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>> {
        for value in values {
            q = match value {
                SqlValue::Null => q.bind(None::<String>),
                SqlValue::Bool(b) => q.bind(*b),
                SqlValue::Int(i) => q.bind(*i),
                SqlValue::Float(f) => q.bind(*f),
                SqlValue::Text(s) => q.bind(s.as_str()),
                // Lists should have been expanded by the compiler into individual
                // placeholders. If we encounter a List here it's a compiler bug.
                SqlValue::List(_) => {
                    // This is a defensive no-op — the compiler should have expanded
                    // lists already. We log a warning and skip.
                    tracing::warn!(
                        "Unexpected List value reached executor — this is a compiler bug"
                    );
                    q
                }
            };
        }
        q
    }

    /// Rewrite generic `?` placeholders to PostgreSQL-style `$1, $2, ...` when needed.
    pub fn normalize_sql(&self, query: &CompiledQuery) -> String {
        query.sql.clone() // Sqlite uses `?` placeholders, so no normalization needed
    }
}

#[async_trait::async_trait]
impl RyxBackend for SqliteBackend {
    /// Execute a compiled query and return all resulting rows as a vector of DecodedRow.
    /// Uses `sqlx::query` to prepare the query, binds parameters, and executes it against the pool.
    /// Usage:
    /// ```
    /// let query = CompiledQuery {
    ///     sql: "SELECT id, name FROM users WHERE age > $1".to_string(),
    ///     values: vec![SqlValue::Int(30)],
    /// };
    /// let rows = backend.__fetch_all(query).await.unwrap();
    /// for row in rows {
    ///     println!("User ID: {}, Name: {}", row.get("id").unwrap(), row.get("name").unwrap());
    /// }
    /// ```
    async fn __fetch_all(&self, query: CompiledQuery) -> RyxResult<Vec<DecodedRow>> {
        let sql = self.normalize_sql(&query);
        let mut q = sqlx::query::<sqlx::Sqlite>(&sql);
        // Bind parameters to the quer
        q = self.bind_values(q, &query.values);
        // Execute the query and return the results
        let rows = q.fetch_all(&self.pool).await.map_err(RyxError::Database)?;

        Ok(decode_rows(&rows, query.base_table.as_deref()))
    }

    /// Execute a compiled query and return a single DecodedRow.
    /// Uses `sqlx::query` to prepare the query, binds parameters, and executes it against the pool.
    /// Usage:
    /// ```
    /// let query = CompiledQuery {
    ///     sql: "SELECT id, name FROM users WHERE id = $1".to_string(),
    ///     values: vec![SqlValue::Int(42)],
    /// };
    /// let row = backend.__fetch_one(query).await.unwrap();
    /// println!("User ID: {}, Name: {}", row.get("id").unwrap(), row.get("name").unwrap());
    /// ```
    async fn __fetch_one(&self, query: CompiledQuery) -> RyxResult<DecodedRow> {
        let mut q = sqlx::query::<sqlx::Sqlite>(&query.sql);
        // Bind parameters to the query
        q = self.bind_values(q, &query.values);
        // Execute the query and return the result
        let row = q.fetch_one(&self.pool).await.map_err(RyxError::Database)?;
        let mapping = std::sync::Arc::new(crate::backends::RowMapping {
            columns: row.columns().iter().map(|c| c.name().to_string()).collect(),
        });

        // Decode the single row into a DecodedRow and return it
        Ok(decode_row(&row, &mapping, query.base_table.as_deref()))
    }

    /// Execute a compiled mutation query (INSERT/UPDATE/DELETE) and return the number of affected rows.
    /// Uses `sqlx::query` to prepare the query, binds parameters, and executes it against the pool.
    /// Usage:
    /// ```
    /// let query = CompiledQuery {
    ///     sql: "UPDATE users SET active = false WHERE last_login < $1".to_string(),
    ///     values: vec![SqlValue::Text("2024-01-01".to_string())],
    /// };
    /// let result = backend.__execute(query).await.unwrap();
    /// println!("Number of users deactivated: {}", result.rows_affected);
    /// ```
    async fn fetch_all(&self, query: CompiledQuery) -> RyxResult<Vec<DecodedRow>> {
        if let Some(tx) = get_current_transaction() {
            let tx_guard = tx.lock().await;
            if let Some(active_tx) = tx_guard.as_ref() {
                return active_tx.fetch_query(query).await;
            }
            return Err(RyxError::Internal("Transaction is no longer active".into()));
        }

        // let pool = pool::get(query.db_alias.as_deref())?.as_any();
        debug!(sql = %query.sql, "Executing SELECT");

        // let sql = self.normalize_sql(&query);
        // let mut q = sqlx::query::<sqlx::Postgres>(&sql);
        // q = self.bind_values(q, &query.values);

        // let rows = q.fetch_all(&self.pool).await.map_err(RyxError::Database)?;
        let rows: Vec<DecodedRow> = self.__fetch_all(query).await?;

        // let decoded = decode_rows(&rows, query.base_table.as_deref());
        Ok(rows)
    }

    /// Execute a raw SQL query and return all resulting rows as a vector of DecodedRow.
    /// This is used for queries that bypass the compiler and are executed directly.
    /// Usage:
    /// ```
    /// let sql = "SELECT id, name FROM users WHERE active = true".to_string();
    /// let rows = backend.fetch_raw(sql, None).await.unwrap();
    /// for row in rows {
    ///     println!("User ID: {}, Name: {}", row.get("id").unwrap(), row.get("name").unwrap());
    /// }
    /// ```
    async fn fetch_raw(
        &self,
        sql: String,
        _db_alias: Option<String>,
    ) -> RyxResult<Vec<DecodedRow>> {
        let rows = sqlx::query::<sqlx::Sqlite>(&sql)
            .fetch_all(&self.pool)
            .await
            .map_err(RyxError::Database)?;
        Ok(decode_rows(&rows, None))
    }

    /// Execute a compiled query represented as a QueryNode and return all resulting rows as a vector of DecodedRow.
    /// This is a convenience method that compiles the QueryNode and then executes it using fetch_all.
    /// Usage:
    /// ```
    /// let node = QueryNode::Select { ... }; // Construct a QueryNode representing the query
    /// let rows = backend.fetch_all_compiled(node).await.unwrap();
    /// for row in rows {
    ///     println!("User ID: {}, Name: {}", row.get("id").unwrap(), row.get("name").unwrap());
    /// }
    /// ```
    async fn fetch_all_compiled(&self, node: QueryNode) -> RyxResult<Vec<DecodedRow>> {
        let compiled = compile(&node).map_err(RyxError::from)?;
        self.__fetch_all(compiled).await
    }

    /// Execute a SELECT COUNT(*) query and return the count.
    ///
    /// # Errors
    /// Same as [`fetch_all`].
    #[instrument(skip(query, self), fields(sql = %query.sql))]
    async fn fetch_count(&self, query: CompiledQuery) -> RyxResult<i64> {
        if let Some(tx) = get_current_transaction() {
            let tx_guard = tx.lock().await;
            if let Some(active_tx) = tx_guard.as_ref() {
                let rows = active_tx.fetch_query(query).await?;
                if rows.is_empty() {
                    return Ok(0);
                }
                if let Some(value) = rows[0].values.first() {
                    match value {
                        SqlValue::Int(i) => return Ok(*i),
                        SqlValue::Float(f) => return Ok(*f as i64),
                        _ => {}
                    }
                }
                return Err(RyxError::Internal(
                    "COUNT() returned unexpected value".into(),
                ));
            }
            return Err(RyxError::Internal("Transaction is no longer active".into()));
        }

        // let pool = pool::get(query.db_alias.as_deref())?.as_any();

        debug!(sql = %query.sql, "Executing COUNT");

        let mut q = sqlx::query::<sqlx::Sqlite>(&query.sql);
        q = self.bind_values(q, &query.values);

        let row = q.fetch_one(&self.pool).await.map_err(RyxError::Database)?;

        let count: i64 = row.try_get(0).unwrap_or_else(|_| {
            let n: i32 = row.try_get(0).unwrap_or(0);
            n as i64
        });

        Ok(count)
    }

    /// Execute a COUNT query represented as a QueryNode and return the count.
    /// This is a convenience method that compiles the QueryNode and then executes it using fetch_count.
    /// # Errors
    /// Same as [`fetch_count`].
    #[instrument(skip(node, self))]
    async fn fetch_count_compiled(&self, node: QueryNode) -> RyxResult<i64> {
        let compiled = compile(&node).map_err(RyxError::from)?;
        self.fetch_count(compiled).await
    }

    /// Execute a SELECT and return at most one row.
    ///
    /// # Errors
    /// - [`RyxError::DoesNotExist`] if no rows are found
    /// - [`RyxError::MultipleObjectsReturned`] if more than one row is found
    ///
    /// This mirrors Django's `.get()` semantics exactly.
    #[instrument(skip(query, self), fields(sql = %query.sql))]
    async fn fetch_one(&self, query: CompiledQuery) -> RyxResult<DecodedRow> {
        // We intentionally fetch up to 2 rows to detect MultipleObjectsReturned
        // without fetching the entire result set. This is more efficient than
        // `fetch_all` when the user calls `.get()` on a large table.
        if let Some(tx) = get_current_transaction() {
            let tx_guard = tx.lock().await;
            if let Some(active_tx) = tx_guard.as_ref() {
                let rows = active_tx.fetch_query(query).await?;
                match rows.len() {
                    0 => Err(RyxError::DoesNotExist),
                    1 => Ok(rows.into_iter().next().unwrap()),
                    _ => Err(RyxError::MultipleObjectsReturned),
                }
            } else {
                Err(RyxError::Internal("Transaction is no longer active".into()))
            }
        } else {
            // let pool = pool::get(query.db_alias.as_deref())?.as_any();

            let sql = self.normalize_sql(&query);
            let mut q = sqlx::query::<sqlx::Sqlite>(&sql);
            q = self.bind_values(q, &query.values);

            // Limit to 2 at the executor level (the QueryNode may already have
            // LIMIT 1 set by `.first()`, but for `.get()` it doesn't).
            // We check the count in Rust rather than adding SQL complexity.
            let rows = q.fetch_all(&self.pool).await.map_err(RyxError::Database)?;
            //self.__fetch_all(query).await?;
            //q.fetch_all(&*pool).await.map_err(RyxError::Database)?;

            let mapping = if rows.is_empty() {
                None
            } else {
                Some(std::sync::Arc::new(crate::backends::RowMapping {
                    columns: rows[0]
                        .columns()
                        .iter()
                        .map(|c| c.name().to_string())
                        .collect(),
                }))
            };

            match rows.len() {
                0 => Err(RyxError::DoesNotExist),
                1 => Ok(decode_row(
                    &rows[0],
                    mapping.as_ref().unwrap(),
                    query.base_table.as_deref(),
                )),
                _ => Err(RyxError::MultipleObjectsReturned),
            }
        }
    }

    /// Execute a SELECT represented as a QueryNode and return at most one row.
    /// This is a convenience method that compiles the QueryNode and then executes it using fetch_one.
    /// # Errors
    /// - [`RyxError::DoesNotExist`] if no rows are found
    /// - [`RyxError::MultipleObjectsReturned`] if more than one row is found
    #[instrument(skip(node, self))]
    async fn fetch_one_compiled(&self, node: QueryNode) -> RyxResult<DecodedRow> {
        let compiled = compile(&node).map_err(RyxError::from)?;
        self.fetch_one(compiled).await
    }

    /// Execute an INSERT, UPDATE, or DELETE query.
    ///
    /// For INSERT queries with `RETURNING` clause, this fetches the returned
    /// value and populates `last_insert_id`.
    ///
    /// # Errors
    /// - [`RyxError::PoolNotInitialized`]
    /// - [`RyxError::Database`]
    #[instrument(skip(query, self), fields(sql = %query.sql))]
    async fn execute(&self, query: CompiledQuery) -> RyxResult<MutationResult> {
        // Check if we're in a transaction and execute there if so,
        // to ensure we stay on the same connection.
        if let Some(tx) = get_current_transaction() {
            let tx_guard = tx.lock().await;
            if let Some(active_tx) = tx_guard.as_ref() {
                // Check if this is a RETURNING query
                if query.sql.to_uppercase().contains("RETURNING") {
                    let rows = active_tx.fetch_query(query).await?;
                    let last_insert_id = rows.first().and_then(|row| {
                        row.values.first().and_then(|v| match v {
                            SqlValue::Int(i) => Some(*i),
                            SqlValue::Float(f) => Some(*f as i64),
                            _ => None,
                        })
                    });
                    return Ok(MutationResult {
                        rows_affected: 1,
                        last_insert_id,
                        returned_ids: Some(
                            rows.iter()
                                .filter_map(|row| {
                                    row.values.first().and_then(|v| match v {
                                        SqlValue::Int(i) => Some(*i),
                                        SqlValue::Float(f) => Some(*f as i64),
                                        _ => None,
                                    })
                                })
                                .collect(),
                        ),
                    });
                }
                let rows_affected = active_tx.execute_query(query).await?;
                return Ok(MutationResult {
                    rows_affected,
                    last_insert_id: None,
                    returned_ids: None,
                });
            }
            return Err(RyxError::Internal("Transaction is no longer active".into()));
        }

        // let pool = pool::get(query.db_alias.as_deref())?.as_any();

        debug!(sql = %query.sql, "Executing mutation");

        // Check if this is a RETURNING query (e.g. INSERT ... RETURNING id)
        let sql = self.normalize_sql(&query);
        if sql.to_uppercase().contains("RETURNING") {
            let mut q = sqlx::query::<sqlx::Sqlite>(&sql);
            q = self.bind_values(q, &query.values);

            let rows = q
                .fetch_all(&self.pool)
                .await
                .map_err(|e| RyxError::DatabaseWithSql(sql.clone(), e))?;

            let last_insert_id = rows.first().and_then(|row| row.try_get::<i64, _>(0).ok());
            let returned_ids: Vec<i64> = rows
                .iter()
                .filter_map(|row| row.try_get::<i64, _>(0).ok())
                .collect();

            return Ok(MutationResult {
                rows_affected: rows.len() as u64,
                last_insert_id,
                returned_ids: Some(returned_ids),
            });
        }

        let mut q = sqlx::query::<sqlx::Sqlite>(&sql);
        q = self.bind_values(q, &query.values);

        let result = q
            .execute(&self.pool)
            .await
            .map_err(|e| RyxError::DatabaseWithSql(sql.clone(), e))?;

        Ok(MutationResult {
            rows_affected: result.rows_affected(),
            last_insert_id: None,
            returned_ids: None,
        })
    }

    /// Execute QueryNode
    #[instrument(skip(node, self))]
    async fn execute_compiled(&self, node: QueryNode) -> RyxResult<MutationResult> {
        let compiled = compile(&node).map_err(RyxError::from)?;
        self.execute(compiled).await
    }

    /// Bulk insert rows with values already mapped to SqlValue in one shot.
    /// This is used for efficient bulk inserts, especially when the data is already in memory and we want to avoid multiple round-trips to the database.
    /// The `returning_id` flag indicates whether to return the last inserted ID(s), which is useful for auto-increment primary keys.
    /// The `ignore_conflicts` flag allows the caller to specify whether to ignore conflicts (e.g. duplicate keys) during insertion, which can be useful for upsert-like behavior.
    /// # Errors
    /// - [`RyxError::PoolNotInitialized`]
    /// - [`RyxError::Database`]
    async fn bulk_insert(
        &self,
        table: String,
        columns: Vec<String>,
        rows: Vec<Vec<SqlValue>>,
        returning_id: bool,
        ignore_conflicts: bool,
        _db_alias: Option<String>,
    ) -> RyxResult<MutationResult> {
        if rows.is_empty() {
            return Ok(MutationResult {
                rows_affected: 0,
                last_insert_id: None,
                returned_ids: None,
            });
        }
        // let pool = pool::get(db_alias.as_deref())?.as_any();
        // let backend = pool::get_backend(db_alias.as_deref())?;

        let col_list = columns
            .iter()
            .map(|c| format!("\"{}\"", c))
            .collect::<Vec<_>>()
            .join(", ");

        // Build placeholders once with proper casting for PostgreSQL.
        let mut placeholders: Vec<String> = Vec::with_capacity(columns.len());
        for (idx, _col) in columns.iter().enumerate() {
            let raw = {
                match rows.get(0).and_then(|r| r.get(idx)) {
                    Some(SqlValue::Text(s)) if is_date(s) => "CAST(? AS DATE)".to_string(),
                    Some(SqlValue::Text(s)) if is_timestamp(s) => {
                        "CAST(? AS TIMESTAMP)".to_string()
                    }
                    _ => "?".to_string(),
                }
            };
            placeholders.push(raw);
        }

        let row_ph = format!("({})", placeholders.join(", "));
        // For PostgreSQL we must bump placeholder numbers per row.
        let mut values_sql_parts = Vec::with_capacity(rows.len());

        values_sql_parts = std::iter::repeat(row_ph.clone()).take(rows.len()).collect();

        let values_sql = values_sql_parts.join(", ");

        let mut flat: SmallVec<[SqlValue; 8]> = SmallVec::new();
        for row in rows {
            for v in row {
                flat.push(v);
            }
        }

        // On confilct
        let (insert_kw, conflict_suffix) = if ignore_conflicts {
            ("INSERT OR IGNORE INTO", "")
        } else {
            ("INSERT INTO", "")
        };

        let sql = format!(
            "{} \"{}\" ({}) VALUES {}{}{}",
            insert_kw,
            table,
            col_list,
            values_sql,
            conflict_suffix,
            if returning_id { " RETURNING id" } else { "" }
        );

        let mut q = sqlx::query::<sqlx::Sqlite>(&sql);
        q = self.bind_values(q, &flat);
        if returning_id {
            let rows = q.fetch_all(&self.pool).await.map_err(RyxError::Database)?;
            let ids: Vec<i64> = rows
                .iter()
                .filter_map(|r| r.try_get::<i64, _>(0).ok())
                .collect();
            let last_insert_id = ids.first().cloned();
            Ok(MutationResult {
                rows_affected: rows.len() as u64,
                last_insert_id,
                returned_ids: Some(ids),
            })
        } else {
            let res = q.execute(&self.pool).await.map_err(RyxError::Database)?;
            Ok(MutationResult {
                rows_affected: res.rows_affected(),
                last_insert_id: Some(res.last_insert_rowid() as i64),
                returned_ids: None,
            })
        }
    }

    /// Bulk delete by primary key values in one shot.
    #[instrument(skip(table, pk_col, pks, self))]
    async fn bulk_delete(
        &self,
        table: String,
        pk_col: String,
        pks: Vec<SqlValue>,
        db_alias: Option<String>,
    ) -> RyxResult<MutationResult> {
        if pks.is_empty() {
            return Ok(MutationResult {
                rows_affected: 0,
                last_insert_id: None,
                returned_ids: None,
            });
        }

        let ph = (0..pks.len())
            .map(|_| "?".to_string())
            .collect::<Vec<_>>()
            .join(", ");

        let sql = format!("DELETE FROM \"{}\" WHERE \"{}\" IN ({})", table, pk_col, ph);
        debug!(
            target: "ryx::bulk_delete",
            db_alias = db_alias.as_deref().unwrap_or("default"),
            params = pks.len(),
            sql_len = sql.len(),
            "bulk_delete compiled"
        );

        let mut q = sqlx::query::<sqlx::Sqlite>(&sql);
        q = self.bind_values(q, &pks);
        let res = q.execute(&self.pool).await.map_err(RyxError::Database)?;
        Ok(MutationResult {
            rows_affected: res.rows_affected(),
            last_insert_id: None,
            returned_ids: None,
        })
    }

    /// Bulk update using CASE WHEN, values already mapped to SqlValue.
    #[instrument(skip(table, pk_col, col_names, field_values, pks, self))]
    async fn bulk_update(
        &self,
        table: String,
        pk_col: String,
        col_names: Vec<String>,
        field_values: Vec<Vec<SqlValue>>,
        pks: Vec<SqlValue>,
        db_alias: Option<String>,
    ) -> RyxResult<MutationResult> {
        // let pool = pool::get(db_alias.as_deref())?;
        // let backend = pool::get_backend(db_alias.as_deref())?;
        let n = pks.len();
        let f = field_values.len();
        if n == 0 || f == 0 {
            return Ok(MutationResult {
                rows_affected: 0,
                last_insert_id: None,
                returned_ids: None,
            });
        }

        let mut case_clauses = Vec::with_capacity(f);
        let mut all_values: SmallVec<[SqlValue; 8]> = SmallVec::with_capacity(n * f * 2 + n);

        // Build CASE clauses with placeholders.
        for (fi, col_name) in col_names.iter().enumerate() {
            let mut case_parts = Vec::with_capacity(n * 3 + 2);
            case_parts.push(format!("\"{}\" = CASE \"{}\"", col_name, pk_col));

            for i in 0..n {
                let when_ph = "?".to_string();
                let then_ph = "?".to_string();

                case_parts.push(format!("WHEN {} THEN {}", when_ph, then_ph));
                all_values.push(pks[i].clone());
                all_values.push(field_values[fi][i].clone());
            }
            case_parts.push("END".to_string());
            case_clauses.push(case_parts.join(" "));
        }

        let pk_placeholders: Vec<String> = (0..n).map(|_| "?".to_string()).collect();

        for pk in &pks {
            all_values.push(pk.clone());
        }

        let sql = format!(
            "UPDATE \"{}\" SET {} WHERE \"{}\" IN ({})",
            table,
            case_clauses.join(", "),
            pk_col,
            pk_placeholders.join(", ")
        );

        debug!(
            target: "ryx::bulk_update",
            db_alias = db_alias.as_deref().unwrap_or("default"),
            rows = n,
            cols = f,
            sql_len = sql.len(),
            params = all_values.len(),
            "bulk_update compiled"
        );

        let mut q = sqlx::query(&sql);
        q = self.bind_values(q, &all_values);
        let res = q.execute(&self.pool).await.map_err(RyxError::Database)?;
        Ok(MutationResult {
            rows_affected: res.rows_affected(),
            last_insert_id: None,
            returned_ids: None,
        })
    }

    /// Execute raw SQL without bind params.
    #[instrument(skip(sql, self))]
    async fn execute_raw(&self, sql: String, _db_alias: Option<String>) -> RyxResult<()> {
        // let pool = pool::get(db_alias.as_deref())?;
        sqlx::query(&sql)
            .execute(&self.pool)
            .await
            .map_err(RyxError::Database)?;
        Ok(())
    }

    fn pool_stats(&self) -> PoolStats {
        PoolStats {
            size: self.pool.size(),
            idle: self.pool.num_idle() as u32,
        }
    }

    fn get_pool(&self) -> RyxPool {
        RyxPool::SQLite(self.pool.clone())
    }
}
