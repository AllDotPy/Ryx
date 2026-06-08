use std::collections::HashMap;
use std::marker::PhantomData;

use ryx_backend::backends::DecodedRow;
use ryx_backend::pool;
use ryx_common::{RyxResult, SqlValue};
use ryx_query::ast::{
    FilterNode, JoinClause, JoinKind, OrderByClause, QNode, QueryNode, QueryOperation,
};
use ryx_query::compiler;
use ryx_query::symbols::Symbol;

use crate::agg::AggExpr;
use crate::cache::CachedQuerySet;
use crate::into_sql::IntoSqlValue;
use crate::stream::QueryStream;
use crate::q::{parse_field_lookup, q_to_qnode, Q};
use crate::row::FromRow;

pub enum FilterArg {
    Field {
        field: String,
        lookup: String,
        value: ryx_query::ast::SqlValue,
    },
    Q(Q),
}

impl<T: IntoSqlValue> From<(&str, T)> for FilterArg {
    fn from((key, value): (&str, T)) -> Self {
        let (col, lookup) = parse_field_lookup(key);
        FilterArg::Field {
            field: col,
            lookup,
            value: value.into_sql_value(),
        }
    }
}

impl From<Q> for FilterArg {
    fn from(q: Q) -> Self {
        FilterArg::Q(q)
    }
}

pub struct QuerySet<T> {
    pub(crate) node: QueryNode,
    pub(crate) _marker: PhantomData<T>,
}

impl<T: FromRow> QuerySet<T> {
    pub fn new(table: &'static str) -> Self {
        let backend = pool::get_backend(None).unwrap_or(ryx_query::Backend::PostgreSQL);
        Self {
            node: QueryNode::select(table).with_backend(backend),
            _marker: PhantomData,
        }
    }

    // === DATABASE ROUTING ===

    /// Route this query to a specific database alias.
    ///
    /// ```ignore
    /// Post::objects().using("replica").filter("active", true).all().await?;
    /// ```
    pub fn using(mut self, alias: &str) -> Self {
        self.node = self.node.with_db_alias(alias.to_string());
        self
    }

    /// Set the database schema for this query (PostgreSQL multi-schema).
    ///
    /// ```ignore
    /// Post::objects().schema("tenant1").all().await?;
    /// ```
    pub fn schema(mut self, schema: &str) -> Self {
        self.node = self.node.with_schema(schema);
        self
    }

    // === FILTERS ===

    /// Add a filter condition.
    ///
    /// Can be called with either:
    /// - A field key and value: `.filter("age__gte", 18)`
    /// - A Q expression: `.filter(Q::or(Q::new("name", "alice"), Q::new("age__gte", 25)))`
    pub fn filter(mut self, arg: impl Into<FilterArg>) -> Self {
        match arg.into() {
            FilterArg::Field {
                field,
                lookup,
                value,
            } => {
                self.node = self.node.with_filter(FilterNode {
                    field: field.as_str().into(),
                    lookup,
                    value,
                    negated: false,
                });
            }
            FilterArg::Q(q) => {
                let qnode = q_to_qnode(q);
                self.node = self.node.with_q(qnode);
            }
        }
        self
    }

    /// Exclude matching rows.
    ///
    /// Can be called with either:
    /// - A field key and value: `.exclude("is_banned", true)`
    /// - A Q expression: `.exclude(Q::or(Q::new("age__lt", 18), Q::new("is_banned", true)))`
    pub fn exclude(mut self, arg: impl Into<FilterArg>) -> Self {
        match arg.into() {
            FilterArg::Field {
                field,
                lookup,
                value,
            } => {
                self.node = self.node.with_filter(FilterNode {
                    field: field.as_str().into(),
                    lookup,
                    value,
                    negated: true,
                });
            }
            FilterArg::Q(q) => {
                let qnode = q_to_qnode(q);
                // Wrap the Q tree in NOT, attach to q_filter
                self.node = self.node.with_q(QNode::Not(Box::new(qnode)));
            }
        }
        self
    }

    // === ORDERING ===

    pub fn order_by(mut self, field: &str) -> Self {
        self.node = self.node.with_order_by(OrderByClause::parse(field));
        self
    }

    pub fn order_by_all(mut self, fields: &[&str]) -> Self {
        for f in fields {
            self.node = self.node.with_order_by(OrderByClause::parse(f));
        }
        self
    }

    // === PAGINATION ===

    pub fn limit(mut self, n: u64) -> Self {
        self.node = self.node.with_limit(n);
        self
    }

    pub fn offset(mut self, n: u64) -> Self {
        self.node = self.node.with_offset(n);
        self
    }

    pub fn distinct(mut self) -> Self {
        self.node.distinct = true;
        self
    }

    // === EXECUTION — SELECT ===

    /// Fetch raw decoded rows (before FromRow mapping).
    /// Used internally by `.all()` and by `CachedQuerySet`.
    pub(crate) async fn fetch_raw_rows(&self) -> RyxResult<Vec<DecodedRow>> {
        let b = pool::get(self.node.db_alias.as_deref())?;
        let compiled = compiler::compile(&self.node)?;
        b.fetch_all(compiled).await
    }

    pub async fn all(self) -> RyxResult<Vec<T>> {
        let has_joins = !self.node.extra_aliases.is_empty();
        let rows = self.fetch_raw_rows().await?;
        if has_joins {
            rows.iter().map(|r| T::from_row_joined(r)).collect()
        } else {
            rows.iter().map(|r| T::from_row(r)).collect()
        }
    }

    pub async fn get(self, field: &str, value: impl IntoSqlValue) -> RyxResult<T> {
        let (col, _lookup) = parse_field_lookup(field);
        let qs = self.filter((col.as_str(), value));
        let b = pool::get(qs.node.db_alias.as_deref())?;
        let compiled = compiler::compile(&qs.node)?;
        let row = b.fetch_one(compiled).await?;
        T::from_row(&row)
    }

    pub async fn first(self) -> RyxResult<Option<T>> {
        let mut qs = self;
        qs.node = qs.node.with_limit(1);
        let b = pool::get(qs.node.db_alias.as_deref())?;
        let compiled = compiler::compile(&qs.node)?;
        let rows = b.fetch_all(compiled).await?;
        match rows.into_iter().next() {
            Some(row) => T::from_row(&row).map(Some),
            None => Ok(None),
        }
    }

    pub async fn count(self) -> RyxResult<i64> {
        let mut count_node = self.node.clone();
        count_node.operation = QueryOperation::Count;
        let b = pool::get(count_node.db_alias.as_deref())?;
        let compiled = compiler::compile(&count_node)?;
        b.fetch_count(compiled).await
    }

    pub async fn exists(self) -> RyxResult<bool> {
        self.count().await.map(|c| c > 0)
    }

    // === EXECUTION — DELETE ===

    pub async fn delete(self) -> RyxResult<u64> {
        let mut del_node = self.node.clone();
        del_node.operation = QueryOperation::Delete;
        let b = pool::get(del_node.db_alias.as_deref())?;
        let res = b.execute_compiled(del_node).await?;
        Ok(res.rows_affected)
    }

    // === EXECUTION — UPDATE ===

    /// Update matching rows.
    ///
    /// ```ignore
    /// let updated = Post::objects()
    ///     .filter("author", "bob")
    ///     .update(vec![("views", 500)])
    ///     .await?;
    /// ```
    pub async fn update<V: IntoSqlValue>(mut self, assignments: Vec<(&str, V)>) -> RyxResult<u64> {
        let sym_vals: Vec<(Symbol, SqlValue)> = assignments
            .into_iter()
            .map(|(field, value)| {
                let (col, _lookup) = parse_field_lookup(field);
                (Symbol::from(col.as_str()), value.into_sql_value())
            })
            .collect();
        self.node.operation = QueryOperation::Update {
            assignments: sym_vals,
        };
        let b = pool::get(self.node.db_alias.as_deref())?;
        let res = b.execute_compiled(self.node).await?;
        Ok(res.rows_affected)
    }

    // === CACHING ===

    /// Enable caching for this query.
    ///
    /// Requires a global cache backend configured via `cache::configure_cache()`.
    ///
    /// ```ignore
    /// use ryx_rs::cache::{configure_cache, MemoryCache};
    /// configure_cache(MemoryCache::new(300, 5000));
    ///
    /// let posts = Post::objects()
    ///     .filter("active", true)
    ///     .cache(60, None)
    ///     .all().await?;
    /// ```
    pub fn cache(self, ttl: u64, key: Option<String>) -> CachedQuerySet<T> {
        CachedQuerySet {
            inner: self,
            ttl,
            explicit_key: key,
        }
    }

    // === STREAMING ===

    /// Create a streaming paginator for this query.
    ///
    /// ```ignore
    /// let mut stream = Post::objects()
    ///     .filter("active", true)
    ///     .order_by("id")
    ///     .stream(100, Some("id"));
    ///
    /// while let Some(chunk) = stream.next_chunk().await? {
    ///     for post in chunk { /* ... */ }
    /// }
    /// ```
    pub fn stream(self, chunk_size: u64, keyset: Option<&str>) -> QueryStream<T> {
        QueryStream::new(self, chunk_size, keyset)
    }

    // === COMPILED SQL (debug) ===

    pub fn sql(&self) -> RyxResult<String> {
        let compiled = compiler::compile(&self.node)?;
        Ok(compiled.sql)
    }

    // === AGGREGATION ===

    /// Execute an aggregate query and return a single row of results.
    ///
    /// ```ignore
    /// use ryx_rs::agg::{count, avg};
    ///
    /// let stats = Post::objects()
    ///     .filter("active", true)
    ///     .aggregate(&[count("total", "id"), avg("avg_views", "views")])
    ///     .await?;
    /// ```
    pub async fn aggregate(self, aggs: &[AggExpr]) -> RyxResult<HashMap<String, SqlValue>> {
        let mut node = self.node.clone();
        node.operation = QueryOperation::Aggregate;
        for agg in aggs {
            node = node.with_annotation(agg.clone().into_ast());
        }
        let b = pool::get(node.db_alias.as_deref())?;
        let compiled = compiler::compile(&node)?;
        let rows = b.fetch_all(compiled).await?;
        if rows.is_empty() {
            return Ok(HashMap::new());
        }
        let row = &rows[0];
        let mut map = HashMap::new();
        for (i, col) in row.mapping.columns.iter().enumerate() {
            if let Some(val) = row.values.get(i) {
                map.insert(col.clone(), val.clone());
            }
        }
        Ok(map)
    }

    // === COLUMN SELECTION ===

    /// Run the query and return rows as maps of column name → value.
    ///
    /// ```ignore
    /// let rows = Post::objects()
    ///     .filter("active", true)
    ///     .values(&["id", "title", "views"])
    ///     .await?;
    /// ```
    pub async fn values(self, columns: &[&str]) -> RyxResult<Vec<HashMap<String, SqlValue>>> {
        let mut node = self.node.clone();
        let syms: Vec<_> = columns.iter().map(|c: &&str| Symbol::from(*c)).collect();
        node.operation = QueryOperation::Select {
            columns: Some(syms),
        };
        let b = pool::get(node.db_alias.as_deref())?;
        let compiled = compiler::compile(&node)?;
        let rows = b.fetch_all(compiled).await?;
        let result = rows
            .iter()
            .map(|row| {
                let mut map = HashMap::new();
                for (i, col) in row.mapping.columns.iter().enumerate() {
                    if let Some(val) = row.values.get(i) {
                        map.insert(col.clone(), val.clone());
                    }
                }
                map
            })
            .collect();
        Ok(result)
    }

    /// Run the query and return rows as lists of values (no column names).
    pub async fn values_list(self, columns: &[&str]) -> RyxResult<Vec<Vec<SqlValue>>> {
        let mut node = self.node.clone();
        let syms: Vec<_> = columns.iter().map(|c| Symbol::from(*c)).collect();
        node.operation = QueryOperation::Select {
            columns: Some(syms),
        };
        let b = pool::get(node.db_alias.as_deref())?;
        let compiled = compiler::compile(&node)?;
        let rows = b.fetch_all(compiled).await?;
        let result = rows
            .iter()
            .map(|row| row.values.clone())
            .collect();
        Ok(result)
    }

    // === ANNOTATE ===

    /// Annotate each row with computed values (aggregates, expressions).
    ///
    /// Selects model fields + annotation columns. Returns rows as maps.
    ///
    /// ```ignore
    /// let rows = Post::objects()
    ///     .annotate(&[count("comment_count", "id")])
    ///     .await?;
    /// // Each row: { "id": 1, "title": "...", "comment_count": 5, ... }
    /// ```
    pub async fn annotate(
        self,
        annotations: &[AggExpr],
    ) -> RyxResult<Vec<HashMap<String, SqlValue>>> {
        let mut node = self.node.clone();
        node.operation = QueryOperation::Select { columns: None };
        for ann in annotations {
            node = node.with_annotation(ann.clone().into_ast());
        }
        let b = pool::get(node.db_alias.as_deref())?;
        let compiled = compiler::compile(&node)?;
        let rows = b.fetch_all(compiled).await?;
        let result = rows
            .iter()
            .map(|row| {
                let mut map = HashMap::new();
                for (i, col) in row.mapping.columns.iter().enumerate() {
                    if let Some(val) = row.values.get(i) {
                        map.insert(col.clone(), val.clone());
                    }
                }
                map
            })
            .collect();
        Ok(result)
    }
}

// === JOIN / SELECT_RELATED (requires Relationships trait) ===

impl<T: FromRow + crate::model::Relationships> QuerySet<T> {
    /// Fetch related models via LEFT OUTER JOIN.
    ///
    /// The relation names must match those defined in `Relationships::relations()`.
    ///
    /// ```ignore
    /// let posts = Post::objects()
    ///     .select_related(&["author"])
    ///     .all().await?;
    /// // Each Post row includes author columns (mapped via FromRow aliases)
    /// ```
    pub fn select_related(mut self, relations: &[&str]) -> Self {
        let all_rels = T::relations();
        let table = T::table_name();
        let mut rel_names_used: Vec<&str> = Vec::new();
        for name in relations {
            if let Some(rel) = all_rels.iter().find(|r| r.name == *name) {
                let join = JoinClause {
                    kind: JoinKind::LeftOuter,
                    table: Symbol::from(rel.to_table),
                    alias: Some(Symbol::from(rel.name)),
                    on_left: format!("{}.{}", table, rel.fk_column),
                    on_right: format!("{}.{}", rel.name, rel.to_field),
                };
                self.node = self.node.with_join(join);
                rel_names_used.push(rel.name);
            }
        }

        // Build explicit main-table column list (qualified, to avoid ambiguity)
        let main_cols: Vec<Symbol> = T::field_names()
            .iter()
            .map(|f| format!("{}.{}", table, f).into())
            .collect();

        // Build extra aliases for each relation's fields:
        //   SELECT "alias"."field" AS "alias__field"
        for rel_name in &rel_names_used {
            if let Some(rel) = all_rels.iter().find(|r| r.name == *rel_name) {
                for field in rel.relation_fields {
                    self.node = self.node.with_extra_alias(
                        format!("{}.{}", rel.name, field),
                        format!("{}__{}", rel.name, field),
                    );
                }
            }
        }

        self.node.operation = QueryOperation::Select {
            columns: Some(main_cols),
        };
        self
    }
}
