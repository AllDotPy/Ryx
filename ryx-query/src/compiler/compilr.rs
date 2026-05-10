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
use crate::symbols::{GLOBAL_INTERNER, Symbol};
use dashmap::DashMap;
use once_cell::sync::Lazy;
use smallvec::SmallVec;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use super::helpers;
pub use super::helpers::{KNOWN_TRANSFORMS, apply_like_wrapping, qualified_col, split_qualified};

/// A specialized buffer for building SQL queries with minimal allocations.
pub struct SqlWriter {
    buf: String,
    emit: bool,
}

impl SqlWriter {
    pub fn new_emit() -> Self {
        Self {
            buf: String::with_capacity(256),
            emit: true,
        }
    }

    pub fn new_no_emit() -> Self {
        Self {
            buf: String::new(),
            emit: false,
        }
    }

    pub fn fork(&self) -> Self {
        Self {
            buf: String::with_capacity(64),
            emit: self.emit,
        }
    }

    fn write(&mut self, s: &str) {
        if self.emit {
            self.buf.push_str(s);
        }
    }

    fn write_quote(&mut self, s: &str) {
        if self.emit {
            self.buf.push('"');
            for c in s.chars() {
                if c == '"' {
                    self.buf.push('"');
                    self.buf.push('"');
                } else {
                    self.buf.push(c);
                }
            }
            self.buf.push('"');
        }
    }

    fn write_symbol(&mut self, sym: crate::symbols::Symbol) {
        let resolved = GLOBAL_INTERNER.resolve(sym);
        self.write_quote(&resolved);
    }

    fn write_qualified(&mut self, s: &str) {
        if let Some((table, col)) = s.split_once('.') {
            self.write_quote(table);
            self.buf.push('.');
            self.write_quote(col);
        } else {
            self.write_quote(s);
        }
    }

    fn write_qualified_symbol(&mut self, sym: crate::symbols::Symbol) {
        let resolved = GLOBAL_INTERNER.resolve(sym);
        self.write_qualified(&resolved);
    }

    fn write_comma_separated<I, F>(&mut self, items: I, f: F)
    where
        I: IntoIterator,
        F: FnMut(I::Item, &mut Self),
    {
        self.write_separated(items, ", ", f);
    }

    fn write_separated<I, F>(&mut self, items: I, sep: &str, mut f: F)
    where
        I: IntoIterator,
        F: FnMut(I::Item, &mut Self),
    {
        let mut first = true;
        for item in items {
            if !first {
                self.buf.push_str(sep);
            }
            f(item, self);
            first = false;
        }
    }

    fn finish(self) -> String {
        self.buf
    }
}

/// Stable hash of the query shape (ignores parameter values).
pub type PlanHash = u64;

#[derive(Clone)]
struct CachedPlan {
    sql: String,
}

static PLAN_CACHE: Lazy<DashMap<PlanHash, CachedPlan>> = Lazy::new(|| DashMap::with_capacity(1024));

#[derive(Debug, Clone)]
pub struct CompiledQuery {
    pub sql: String,
    pub values: SmallVec<[SqlValue; 8]>,
    pub db_alias: Option<String>,
    pub base_table: Option<String>,
    pub column_names: Option<Vec<String>>,
    pub backend: Backend,
}

pub fn compile(node: &QueryNode) -> QueryResult<CompiledQuery> {
    let mut values: SmallVec<[SqlValue; 8]> = SmallVec::new();
    let plan_hash = compute_plan_hash(node);
    let mut node_column_names: Option<Vec<String>> = None;
    let mut writer = if PLAN_CACHE.contains_key(&plan_hash) {
        SqlWriter::new_no_emit()
    } else {
        SqlWriter::new_emit()
    };

    match &node.operation {
        QueryOperation::Select { columns } => {
            compile_select(node, columns.as_deref(), &mut values, &mut writer)?;
        }
        QueryOperation::Aggregate => compile_aggregate(node, &mut values, &mut writer)?,
        QueryOperation::Count => compile_count(node, &mut values, &mut writer)?,
        QueryOperation::Delete => compile_delete(node, &mut values, &mut writer)?,
        QueryOperation::Update { assignments } => {
            let cols = compile_update(node, assignments, &mut values, &mut writer)?;
            node_column_names = Some(cols);
        }
        QueryOperation::Insert {
            values: cv,
            returning_id,
        } => {
            let cols = compile_insert(node, cv, *returning_id, &mut values, &mut writer)?;
            node_column_names = Some(cols);
        }
    };

    // Now get the sql from the cache if exixts
    let sql = if let Some(cached) = PLAN_CACHE.get(&plan_hash) {
        cached.sql.clone()
    } else {
        // Save final sql to the cache.
        let sql = writer.finish();
        PLAN_CACHE.insert(plan_hash, CachedPlan { sql: sql.clone() });
        sql
    };
    Ok(CompiledQuery {
        sql,
        values,
        db_alias: node.db_alias.clone(),
        base_table: Some(GLOBAL_INTERNER.resolve(node.table)),
        column_names: node_column_names,
        backend: node.backend,
    })
}

fn compute_plan_hash(node: &QueryNode) -> PlanHash {
    let mut h = DefaultHasher::new();
    node.table.hash(&mut h);
    node.backend.hash(&mut h);
    node.distinct.hash(&mut h);
    node.limit.hash(&mut h);
    node.offset.hash(&mut h);
    for ob in &node.order_by {
        ob.field.hash(&mut h);
        ob.direction.hash(&mut h);
    }
    for gb in &node.group_by {
        gb.hash(&mut h);
    }
    for j in &node.joins {
        j.kind.hash(&mut h);
        j.table.hash(&mut h);
        j.alias.hash(&mut h);
        j.on_left.hash(&mut h);
        j.on_right.hash(&mut h);
    }
    for f in &node.filters {
        f.field.hash(&mut h);
        f.lookup.hash(&mut h);
        f.negated.hash(&mut h);
    }
    if let Some(q) = &node.q_filter {
        hash_q(q, &mut h);
    }
    for a in &node.annotations {
        a.alias.hash(&mut h);
        a.func.sql_name().hash(&mut h);
        a.field.hash(&mut h);
        a.distinct.hash(&mut h);
    }
    match &node.operation {
        QueryOperation::Select { columns } => {
            1u8.hash(&mut h);
            if let Some(cols) = columns {
                for c in cols {
                    c.hash(&mut h);
                }
            }
        }
        QueryOperation::Aggregate => 2u8.hash(&mut h),
        QueryOperation::Count => 3u8.hash(&mut h),
        QueryOperation::Delete => 4u8.hash(&mut h),
        QueryOperation::Update { assignments } => {
            5u8.hash(&mut h);
            for (col, _) in assignments {
                col.hash(&mut h);
            }
        }
        QueryOperation::Insert {
            values,
            returning_id,
        } => {
            6u8.hash(&mut h);
            returning_id.hash(&mut h);
            for (col, _) in values {
                col.hash(&mut h);
            }
        }
    }
    h.finish()
}

fn hash_q(q: &QNode, h: &mut DefaultHasher) {
    match q {
        QNode::Leaf {
            field,
            lookup,
            negated,
            ..
        } => {
            1u8.hash(h);
            field.hash(h);
            lookup.hash(h);
            negated.hash(h);
        }
        QNode::And(children) => {
            2u8.hash(h);
            for c in children {
                hash_q(c, h);
            }
        }
        QNode::Or(children) => {
            3u8.hash(h);
            for c in children {
                hash_q(c, h);
            }
        }
        QNode::Not(child) => {
            4u8.hash(h);
            hash_q(child, h);
        }
    }
}

fn compile_select(
    node: &QueryNode,
    columns: Option<&[Symbol]>,
    values: &mut SmallVec<[SqlValue; 8]>,
    writer: &mut SqlWriter,
) -> QueryResult<()> {
    let distinct = if node.distinct { "DISTINCT " } else { "" };
    writer.write("SELECT ");
    writer.write(distinct);

    if columns.is_none() || columns.is_some_and(|c| c.is_empty()) {
        if node.annotations.is_empty() {
            writer.write("*");
        } else {
            if node.group_by.is_empty() {
                compile_agg_cols(&node.annotations, writer);
            } else {
                writer.write_comma_separated(&node.group_by, |c, w| w.write_symbol(*c));
                writer.write(", ");
                compile_agg_cols(&node.annotations, writer);
            }
        }
    } else {
        let cols = columns.unwrap();
        writer.write_comma_separated(cols, |c, w| w.write_qualified_symbol(*c));
        if !node.annotations.is_empty() {
            writer.write(", ");
            compile_agg_cols(&node.annotations, writer);
        }
    }

    writer.write(" FROM ");
    writer.write_symbol(node.table);

    if !node.joins.is_empty() {
        writer.write(" ");
        compile_joins(&node.joins, writer);
    }

    compile_where_combined(
        &node.filters,
        node.q_filter.as_ref(),
        values,
        node.backend,
        writer,
    )?;

    if !node.group_by.is_empty() {
        writer.write(" GROUP BY ");
        writer.write_comma_separated(&node.group_by, |c, w| w.write_symbol(*c));
    }

    if !node.having.is_empty() {
        writer.write(" HAVING ");
        compile_filters(&node.having, values, node.backend, writer)?;
    }

    if !node.order_by.is_empty() {
        writer.write(" ORDER BY ");
        compile_order_by(&node.order_by, writer);
    }

    if let Some(n) = node.limit {
        writer.write(" LIMIT ");
        writer.write(&n.to_string());
    }
    if let Some(n) = node.offset {
        writer.write(" OFFSET ");
        writer.write(&n.to_string());
    }

    Ok(())
}

fn compile_aggregate(
    node: &QueryNode,
    values: &mut SmallVec<[SqlValue; 8]>,
    writer: &mut SqlWriter,
) -> QueryResult<()> {
    if node.annotations.is_empty() {
        return Err(QueryError::Internal(
            "aggregate() called with no aggregate expressions".into(),
        ));
    }
    writer.write("SELECT ");
    compile_agg_cols(&node.annotations, writer);
    writer.write(" FROM ");
    let table_resolved = GLOBAL_INTERNER.resolve(node.table);
    writer.write_quote(&table_resolved);

    if !node.joins.is_empty() {
        writer.write(" ");
        compile_joins(&node.joins, writer);
    }

    compile_where_combined(
        &node.filters,
        node.q_filter.as_ref(),
        values,
        node.backend,
        writer,
    )?;

    Ok(())
}

fn compile_count(
    node: &QueryNode,
    values: &mut SmallVec<[SqlValue; 8]>,
    writer: &mut SqlWriter,
) -> QueryResult<()> {
    writer.write("SELECT COUNT(*) FROM ");
    let table_resolved = GLOBAL_INTERNER.resolve(node.table);
    writer.write_quote(&table_resolved);
    if !node.joins.is_empty() {
        writer.write(" ");
        compile_joins(&node.joins, writer);
    }
    compile_where_combined(
        &node.filters,
        node.q_filter.as_ref(),
        values,
        node.backend,
        writer,
    )?;
    Ok(())
}

fn compile_delete(
    node: &QueryNode,
    values: &mut SmallVec<[SqlValue; 8]>,
    writer: &mut SqlWriter,
) -> QueryResult<()> {
    writer.write("DELETE FROM ");
    let table_resolved = GLOBAL_INTERNER.resolve(node.table);
    writer.write_quote(&table_resolved);
    compile_where_combined(
        &node.filters,
        node.q_filter.as_ref(),
        values,
        node.backend,
        writer,
    )?;
    Ok(())
}

fn compile_update(
    node: &QueryNode,
    assignments: &[(Symbol, SqlValue)],
    values: &mut SmallVec<[SqlValue; 8]>,
    writer: &mut SqlWriter,
) -> QueryResult<Vec<String>> {
    if assignments.is_empty() {
        return Err(QueryError::Internal("UPDATE with no assignments".into()));
    }
    writer.write("UPDATE ");
    let table_resolved = GLOBAL_INTERNER.resolve(node.table);
    writer.write_quote(&table_resolved);
    writer.write(" SET ");

    let mut cols_out: Vec<String> = Vec::with_capacity(assignments.len());
    writer.write_comma_separated(assignments, |(col, val), w| {
        values.push(val.clone());
        let resolved = GLOBAL_INTERNER.resolve(*col);
        cols_out.push(resolved.clone());
        w.write_quote(&resolved);
        w.write(" = ?");
    });

    compile_where_combined(
        &node.filters,
        node.q_filter.as_ref(),
        values,
        node.backend,
        writer,
    )?;
    Ok(cols_out)
}

fn compile_insert(
    node: &QueryNode,
    cols_vals: &[(Symbol, SqlValue)],
    returning_id: bool,
    values: &mut SmallVec<[SqlValue; 8]>,
    writer: &mut SqlWriter,
) -> QueryResult<Vec<String>> {
    // Ensure values are provided and extract column names and values.
    if cols_vals.is_empty() {
        return Err(QueryError::Internal("INSERT with no values".into()));
    }

    let (cols, vals): (Vec<_>, Vec<_>) = cols_vals.iter().cloned().unzip();
    values.extend(vals);

    writer.write("INSERT INTO ");
    let table_resolved = GLOBAL_INTERNER.resolve(node.table);
    writer.write_quote(&table_resolved);
    writer.write(" (");
    writer.write_comma_separated(&cols, |c, w| w.write_symbol(*c));
    writer.write(") VALUES (");
    for i in 0..cols.len() {
        writer.write("?");
        if i < cols.len() - 1 {
            writer.write(", ");
        }
    }
    writer.write(")");
    if returning_id {
        writer.write(" RETURNING id");
    }
    let cols_resolved: Vec<String> = cols.iter().map(|s| GLOBAL_INTERNER.resolve(*s)).collect();
    Ok(cols_resolved)
}

pub fn compile_joins(joins: &[JoinClause], writer: &mut SqlWriter) {
    for (i, j) in joins.iter().enumerate() {
        if i > 0 {
            writer.write(" ");
        }
        let kind = match j.kind {
            JoinKind::Inner => "INNER JOIN",
            JoinKind::LeftOuter => "LEFT OUTER JOIN",
            JoinKind::RightOuter => "RIGHT OUTER JOIN",
            JoinKind::FullOuter => "FULL OUTER JOIN",
            JoinKind::CrossJoin => "CROSS JOIN",
        };
        writer.write(kind);
        writer.write(" ");
        writer.write_symbol(j.table);
        if let Some(alias) = &j.alias {
            writer.write(" AS ");
            writer.write_symbol(*alias);
        }

        if j.kind != JoinKind::CrossJoin {
            writer.write(" ON ");
            let (l_table, l_col): (String, String) = helpers::split_qualified(&j.on_left);
            if l_table.is_empty() {
                writer.write_quote(&l_col);
            } else {
                writer.write_quote(&l_table);
                writer.write(".");
                writer.write_quote(&l_col);
            }
            writer.write(" = ");
            let (r_table, r_col): (String, String) = helpers::split_qualified(&j.on_right);
            if r_table.is_empty() {
                writer.write_quote(&r_col);
            } else {
                writer.write_quote(&r_table);
                writer.write(".");
                writer.write_quote(&r_col);
            }
        }
    }
}

pub fn compile_agg_cols(anns: &[AggregateExpr], writer: &mut SqlWriter) {
    writer.write_comma_separated(anns, |a, w| {
        let field_resolved = GLOBAL_INTERNER.resolve(a.field);
        let col = if field_resolved == "*" {
            "*".to_string()
        } else {
            helpers::qualified_col(&field_resolved)
        };
        let distinct = if a.distinct && a.func != AggFunc::Count {
            "DISTINCT "
        } else if a.distinct {
            "DISTINCT "
        } else {
            ""
        };
        match &a.func {
            AggFunc::Raw(expr) => {
                w.write(expr);
                w.write(" AS ");
                w.write_symbol(a.alias);
            }
            f => {
                w.write(f.sql_name());
                w.write("(");
                w.write(distinct);
                if col == "*" {
                    w.write("*");
                } else {
                    w.write_qualified(&col);
                }
                w.write(") AS ");
                w.write_symbol(a.alias);
            }
        }
    });
}

pub fn compile_order_by(clauses: &[crate::ast::OrderByClause], writer: &mut SqlWriter) {
    writer.write_comma_separated(clauses, |c, w| {
        w.write_qualified_symbol(c.field);
        w.write(" ");
        let dir = match c.direction {
            SortDirection::Asc => "ASC",
            SortDirection::Desc => "DESC",
        };
        w.write(dir);
    });
}

fn compile_where_combined(
    filters: &[FilterNode],
    q: Option<&QNode>,
    values: &mut SmallVec<[SqlValue; 8]>,
    backend: Backend,
    writer: &mut SqlWriter,
) -> QueryResult<()> {
    if filters.is_empty() && q.is_none() {
        return Ok(());
    }
    writer.write(" WHERE ");
    let mut has_flat = false;
    if !filters.is_empty() {
        has_flat = true;
        writer.write("(");
        compile_filters(filters, values, backend, writer)?;
        writer.write(")");
    }
    if let Some(q) = q {
        if has_flat {
            writer.write(" AND ");
        }
        writer.write("(");
        compile_q(q, values, backend, writer)?;
        writer.write(")");
    }
    Ok(())
}

pub fn compile_q(
    q: &QNode,
    values: &mut SmallVec<[SqlValue; 8]>,
    backend: Backend,
    writer: &mut SqlWriter,
) -> QueryResult<()> {
    match q {
        QNode::Leaf {
            field,
            lookup,
            value,
            negated,
        } => compile_single_filter(*field, lookup, value, *negated, values, backend, writer),
        QNode::And(children) => {
            writer.write("(");
            writer.write_separated(children, " AND ", |c, w| {
                let mut child_writer = w.fork();
                compile_q(c, values, backend, &mut child_writer).unwrap();
                w.write(&child_writer.finish());
            });
            writer.write(")");
            Ok(())
        }
        QNode::Or(children) => {
            writer.write("(");
            writer.write_separated(children, " OR ", |c, w| {
                let mut child_writer = w.fork();
                compile_q(c, values, backend, &mut child_writer).unwrap();
                w.write(&child_writer.finish());
            });
            writer.write(")");
            Ok(())
        }
        QNode::Not(child) => {
            writer.write("NOT (");
            let mut child_writer = writer.fork();
            compile_q(child, values, backend, &mut child_writer)?;
            writer.write(&child_writer.finish());
            writer.write(")");
            Ok(())
        }
    }
}

fn compile_filters(
    filters: &[FilterNode],
    values: &mut SmallVec<[SqlValue; 8]>,
    backend: Backend,
    writer: &mut SqlWriter,
) -> QueryResult<()> {
    writer.write_separated(filters, " AND ", |f, w| {
        compile_single_filter(f.field, &f.lookup, &f.value, f.negated, values, backend, w).unwrap();
    });
    Ok(())
}

fn compile_single_filter(
    field: Symbol,
    lookup: &str,
    value: &SqlValue,
    negated: bool,
    values: &mut SmallVec<[SqlValue; 8]>,
    backend: Backend,
    writer: &mut SqlWriter,
) -> QueryResult<()> {
    let field_resolved = GLOBAL_INTERNER.resolve(field);
    let (base_column, applied_transforms, json_key) = if field_resolved.contains("__") {
        let parts: Vec<&str> = field_resolved.split("__").collect();

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
        (field_resolved.to_string(), vec![], None)
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
        if negated {
            writer.write("NOT (");
        }
        if is_null {
            writer.write(&final_column);
            writer.write(" IS NULL");
        } else {
            writer.write(&final_column);
            writer.write(" IS NOT NULL");
        }
        if negated {
            writer.write(")");
        }
        return Ok(());
    }

    if lookup == "in" {
        let items: SmallVec<[SqlValue; 4]> = match value {
            SqlValue::List(v) => v.iter().map(|x| (**x).clone()).collect(),
            other => smallvec::smallvec![(*other).clone()],
        };
        if items.is_empty() {
            writer.write("(1 = 0)");
            return Ok(());
        }

        if negated {
            writer.write("NOT (");
        }
        writer.write(&final_column);
        writer.write(" IN (");
        writer.write_separated(&items, ", ", |_, w| w.write("?"));
        writer.write(")");
        if negated {
            writer.write(")");
        }
        values.extend(items);
        return Ok(());
    }

    if lookup == "has_any" || lookup == "has_all" {
        let items: SmallVec<[SqlValue; 4]> = match value {
            SqlValue::List(v) => v.iter().map(|x| (**x).clone()).collect(),
            other => smallvec::smallvec![(*other).clone()],
        };
        if items.is_empty() {
            writer.write("(1 = 0)");
            return Ok(());
        }

        if negated {
            writer.write("NOT (");
        }
        if backend == Backend::PostgreSQL {
            let op = if lookup == "has_any" { "?|" } else { "?&" };
            writer.write(&final_column);
            writer.write(" ");
            writer.write(op);
            writer.write(" ?");
        } else if backend == Backend::MySQL {
            let op = if lookup == "has_any" {
                "'one'"
            } else {
                "'all'"
            };
            writer.write("JSON_CONTAINS_PATH(");
            writer.write(&final_column);
            writer.write(", ");
            writer.write(op);
            writer.write(", ");
            writer.write_separated(&items, ", ", |_, w| {
                w.write("CONCAT('$.', ?)");
            });
            writer.write(")");
        } else {
            // SQLite: manual expansion
            let op = if lookup == "has_any" { " OR " } else { " AND " };
            writer.write_separated(&items, op, |_, w| {
                w.write("json_extract(");
                w.write(&final_column);
                w.write(", '$.' || ?)");
                w.write(" IS NOT NULL");
            });
        }
        if negated {
            writer.write(")");
        }
        values.extend(items);
        return Ok(());
    }

    if lookup == "range" {
        let (lo, hi) = match value {
            SqlValue::List(v) if v.len() == 2 => (v[0].as_ref().clone(), v[1].as_ref().clone()),
            _ => return Err(QueryError::Internal("range needs exactly 2 values".into())),
        };
        if negated {
            writer.write("NOT (");
        }
        writer.write(&final_column);
        writer.write(" BETWEEN ? AND ?");
        if negated {
            writer.write(")");
        }
        values.push(lo);
        values.push(hi);
        return Ok(());
    }

    if lookup.contains("__") || json_key.is_some() {
        if negated {
            writer.write("NOT (");
        }
        let fragment = lookups::resolve(&base_column, lookup, &ctx)?;
        writer.write(&fragment);
        if negated {
            writer.write(")");
        }
        values.push(value.clone());
        return Ok(());
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
                    field: field_resolved.clone(),
                    lookup: lookup.to_string(),
                });
            }
        };
        if negated {
            writer.write("NOT (");
        }
        writer.write(&transform_fn(&ctx));
        if negated {
            writer.write(")");
        }
        values.push(value.clone());
        return Ok(());
    }

    let fragment = lookups::resolve(&base_column, lookup, &ctx)?;
    let bound = apply_like_wrapping(lookup, value.clone());
    if negated {
        writer.write("NOT (");
    }
    writer.write(&fragment);
    if negated {
        writer.write(")");
    }
    values.push(bound);
    Ok(())
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
            .with_group_by("status");
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
            .with_group_by("author_id")
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
