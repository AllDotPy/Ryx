//
// ###
// Ryx — Lookups Implementation
// ###
//
// Contains core types, registry, and resolve logic for the lookup system.
// This is the main implementation file - mod.rs just re-exports from here.
// ###

use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

use crate::errors::{RyxError, RyxResult};
use crate::pool::Backend;

// Re-export submodules
pub use crate::query::lookups::common_lookups;
pub use crate::query::lookups::date_lookups;
pub use crate::query::lookups::json_lookups;

// ###
// Core types
// ###

#[derive(Debug, Clone)]
pub struct LookupContext {
    pub column: String,
    pub negated: bool,
    pub backend: Backend,
    pub json_key: Option<String>,
}

pub type LookupFn = fn(&LookupContext) -> String;

#[derive(Debug, Clone)]
pub struct PythonLookup {
    pub sql_template: String,
}

// ###
// Global lookup registry
// ###

struct LookupRegistry {
    builtin: HashMap<&'static str, LookupFn>,
    custom: HashMap<String, PythonLookup>,
}

static REGISTRY: OnceLock<RwLock<LookupRegistry>> = OnceLock::new();

pub fn init_registry() {
    REGISTRY.get_or_init(|| {
        let mut builtin = HashMap::new();

        builtin.insert("exact", common_lookups::exact as LookupFn);
        builtin.insert("gt", common_lookups::gt as LookupFn);
        builtin.insert("gte", common_lookups::gte as LookupFn);
        builtin.insert("lt", common_lookups::lt as LookupFn);
        builtin.insert("lte", common_lookups::lte as LookupFn);

        builtin.insert("contains", common_lookups::contains as LookupFn);
        builtin.insert("icontains", common_lookups::icontains as LookupFn);
        builtin.insert("startswith", common_lookups::startswith as LookupFn);
        builtin.insert("istartswith", common_lookups::istartswith as LookupFn);
        builtin.insert("endswith", common_lookups::endswith as LookupFn);
        builtin.insert("iendswith", common_lookups::iendswith as LookupFn);

        builtin.insert("isnull", common_lookups::isnull as LookupFn);
        builtin.insert("in", common_lookups::in_lookup as LookupFn);
        builtin.insert("range", common_lookups::range as LookupFn);

        builtin.insert("date", date_lookups::date_transform as LookupFn);
        builtin.insert("year", date_lookups::year_transform as LookupFn);
        builtin.insert("month", date_lookups::month_transform as LookupFn);
        builtin.insert("day", date_lookups::day_transform as LookupFn);
        builtin.insert("hour", date_lookups::hour_transform as LookupFn);
        builtin.insert("minute", date_lookups::minute_transform as LookupFn);
        builtin.insert("second", date_lookups::second_transform as LookupFn);
        builtin.insert("week", date_lookups::week_transform as LookupFn);
        builtin.insert("dow", date_lookups::dow_transform as LookupFn);
        builtin.insert("quarter", date_lookups::quarter_transform as LookupFn);
        builtin.insert("time", date_lookups::time_transform as LookupFn);
        builtin.insert("iso_week", date_lookups::iso_week_transform as LookupFn);
        builtin.insert("iso_dow", date_lookups::iso_dow_transform as LookupFn);

        builtin.insert("key", json_lookups::json_key_transform as LookupFn);
        builtin.insert(
            "key_text",
            json_lookups::json_key_text_transform as LookupFn,
        );
        builtin.insert("json", json_lookups::json_cast_transform as LookupFn);

        builtin.insert("has_key", json_lookups::json_has_key as LookupFn);
        builtin.insert("has_any", json_lookups::json_has_any as LookupFn);
        builtin.insert("has_all", json_lookups::json_has_all as LookupFn);
        builtin.insert("contains", json_lookups::json_contains as LookupFn);
        builtin.insert("contained_by", json_lookups::json_contained_by as LookupFn);
        builtin.insert("has_all", json_lookups::json_has_all as LookupFn);
        builtin.insert("has_any", json_lookups::json_has_any as LookupFn);

        RwLock::new(LookupRegistry {
            builtin,
            custom: HashMap::new(),
        })
    });
}

// ###
// Registry public API
// ###

pub fn register_custom(name: impl Into<String>, sql_template: impl Into<String>) -> RyxResult<()> {
    let registry = REGISTRY
        .get()
        .ok_or_else(|| RyxError::Internal("Lookup registry not initialized".into()))?;

    let mut guard = registry
        .write()
        .map_err(|e| RyxError::Internal(format!("Registry lock poisoned: {e}")))?;

    guard.custom.insert(
        name.into(),
        PythonLookup {
            sql_template: sql_template.into(),
        },
    );

    Ok(())
}

fn resolve_simple(field: &str, lookup_name: &str, ctx: &LookupContext) -> RyxResult<String> {
    let registry = REGISTRY
        .get()
        .ok_or_else(|| RyxError::Internal("Lookup registry not initialized".into()))?;

    let guard = registry
        .read()
        .map_err(|e| RyxError::Internal(format!("Registry lock poisoned: {e}")))?;

    if let Some(custom) = guard.custom.get(lookup_name) {
        return Ok(custom.sql_template.replace("{col}", &ctx.column));
    }

    if let Some(lookup_fn) = guard.builtin.get(lookup_name) {
        return Ok(lookup_fn(ctx));
    }

    Err(RyxError::UnknownLookup {
        field: field.to_string(),
        lookup: lookup_name.to_string(),
    })
}

/// Returns the list of all registered lookup names (built-in + custom).
/// Used by the Python layer for available_lookups().
pub fn registered_lookups() -> RyxResult<Vec<String>> {
    let registry = REGISTRY
        .get()
        .ok_or_else(|| RyxError::Internal("Lookup registry not initialized".into()))?;

    let guard = registry
        .read()
        .map_err(|e| RyxError::Internal(format!("Registry lock poisoned: {e}")))?;

    let mut names: Vec<String> = guard
        .builtin
        .keys()
        .copied()
        .map(|k| k.to_string())
        .chain(guard.custom.keys().cloned())
        .collect();
    names.sort();
    Ok(names)
}

/// Returns a static slice of all built-in lookup names.
/// This is used for auto-discovery on the Python side.
pub fn all_lookups() -> &'static [&'static str] {
    &[
        // Comparison
        "exact",
        "gt",
        "gte",
        "lt",
        "lte",
        // String
        "contains",
        "icontains",
        "startswith",
        "istartswith",
        "endswith",
        "iendswith",
        // Null
        "isnull",
        // Membership
        "in",
        // Range
        "range",
        // Date/Time transforms
        "date",
        "year",
        "month",
        "day",
        "hour",
        "minute",
        "second",
        "week",
        "dow",
        "quarter",
        "time",
        "iso_week",
        "iso_dow",
        // JSON transforms
        "key",
        "key_text",
        "json",
        // JSON lookups
        "has_key",
        "has_any",
        "has_all",
        "contains",
        "contained_by",
    ]
}

/// Returns a static slice of all transform names (date/time + JSON).
/// Used for validation when chaining field transforms.
pub fn all_transforms() -> &'static [&'static str] {
    &[
        "date", "year", "month", "day", "hour", "minute", "second", "week", "dow", "quarter",
        "time", "iso_week", "iso_dow", "key", "key_text", "json",
    ]
}

// ###
// Chained lookups support
// ###

#[allow(dead_code)]
fn handle_sqlite_transform_lookup(
    field: &str,
    _transform: &str,
    lookup_name: &str,
    ctx: &LookupContext,
) -> RyxResult<String> {
    let is_numeric_comparison = matches!(lookup_name, "gt" | "gte" | "lt" | "lte" | "exact");

    if is_numeric_comparison && ctx.column.contains("AS TEXT)") {
        let transformed = ctx.column.replace("AS TEXT)", "AS INTEGER)");
        let new_ctx = LookupContext {
            column: transformed,
            negated: ctx.negated,
            backend: ctx.backend,
            json_key: ctx.json_key.clone(),
        };
        return resolve_simple(field, lookup_name, &new_ctx);
    }

    resolve_simple(field, lookup_name, ctx)
}

fn add_sqlite_cast_for_transform(fragment: &str, lookup: &str) -> String {
    match lookup {
        "exact" => fragment.replace("= ?", "= CAST(? AS TEXT)"),
        "gt" => fragment.replace("> ?", "> CAST(? AS TEXT)"),
        "gte" => fragment.replace(">= ?", ">= CAST(? AS TEXT)"),
        "lt" => fragment.replace("< ?", "< CAST(? AS TEXT)"),
        "lte" => fragment.replace("<= ?", "<= CAST(? AS TEXT)"),
        _ => fragment.to_string(),
    }
}

pub fn resolve(field: &str, lookup_name: &str, ctx: &LookupContext) -> RyxResult<String> {
    if !lookup_name.contains("__") {
        if ctx.json_key.is_some() {
            let mut column = format!("\"{}\"", field);
            column = apply_transform("key", &column, ctx.backend, ctx.json_key.as_deref())?;

            let json_ctx = LookupContext {
                column: column.clone(),
                negated: ctx.negated,
                backend: ctx.backend,
                json_key: None,
            };
            return resolve_simple(field, lookup_name, &json_ctx);
        }

        if ctx.column.contains("strftime") || ctx.column.contains("DATE(") {
            if ctx.column.contains("strftime('%Y'") {
                return handle_sqlite_transform_lookup(field, "year", lookup_name, ctx);
            } else if ctx.column.contains("strftime('%m'") {
                return handle_sqlite_transform_lookup(field, "month", lookup_name, ctx);
            } else if ctx.column.contains("strftime('%d'") {
                return handle_sqlite_transform_lookup(field, "day", lookup_name, ctx);
            } else if ctx.column.contains("strftime('%H'") {
                return handle_sqlite_transform_lookup(field, "hour", lookup_name, ctx);
            }
            if ctx.column.starts_with("DATE(") {
                return resolve_simple(field, lookup_name, ctx);
            }
        }
        return resolve_simple(field, lookup_name, ctx);
    }

    let parts: Vec<&str> = lookup_name.split("__").collect();
    let final_lookup = *parts.last().unwrap();
    let transform_parts: Vec<&str> = parts[..parts.len() - 1].to_vec();

    let mut column = format!("\"{}\"", field);

    for transform in transform_parts.iter() {
        let is_transform = matches!(
            *transform,
            "date"
                | "year"
                | "month"
                | "day"
                | "hour"
                | "minute"
                | "second"
                | "week"
                | "dow"
                | "quarter"
                | "time"
                | "iso_week"
                | "iso_dow"
                | "key"
                | "key_text"
                | "json"
        );

        if is_transform {
            let key = if matches!(*transform, "key" | "key_text") {
                ctx.json_key
                    .as_deref()
                    .or_else(|| field.rsplit("__").next())
            } else {
                None
            };
            column = apply_transform(transform, &column, ctx.backend, key)?;
        } else {
            break;
        }
    }

    let final_ctx = LookupContext {
        column: column.clone(),
        negated: ctx.negated,
        backend: ctx.backend,
        json_key: ctx.json_key.clone(),
    };

    if ctx.backend == Backend::SQLite {
        let col_has_transform = column.contains("strftime");

        if col_has_transform && !column.contains("AS INTEGER") {
            let is_numeric_comparison =
                matches!(final_lookup, "gt" | "gte" | "lt" | "lte" | "exact");

            if is_numeric_comparison {
                let transformed = column.replace("AS TEXT)", "AS INTEGER)");
                let final_ctx_int = LookupContext {
                    column: transformed,
                    negated: ctx.negated,
                    backend: ctx.backend,
                    json_key: ctx.json_key.clone(),
                };
                return resolve_simple(field, final_lookup, &final_ctx_int);
            }

            let fragment = resolve_simple(field, final_lookup, &final_ctx)?;
            return Ok(add_sqlite_cast_for_transform(&fragment, final_lookup));
        }
    }

    resolve_simple(field, final_lookup, &final_ctx)
}

pub fn apply_transform(
    name: &str,
    column: &str,
    backend: Backend,
    key: Option<&str>,
) -> RyxResult<String> {
    if let Some(sql) = date_lookups::apply_date_transform(name, column, backend) {
        return Ok(sql);
    }
    if let Some(sql) = json_lookups::apply_json_transform(name, column, backend, key) {
        return Ok(sql);
    }

    if name == "date" {
        return Ok(format!("DATE({})", column));
    }

    Err(RyxError::UnknownLookup {
        field: column.to_string(),
        lookup: name.to_string(),
    })
}
