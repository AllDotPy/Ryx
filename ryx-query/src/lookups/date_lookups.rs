//
// ###
// Ryx — Date/Time Lookups
// ###
//
// Contains date/time transforms (year, month, day, hour, etc.) and apply_transform logic.
// These are used for chained lookups like `created_at__year__gte=2024`
// ###

use crate::backend::Backend;
use crate::lookups::LookupContext;

pub use crate::lookups::LookupFn;

/// Apply a date/time field transformation.
/// Returns SQL like "DATE(col)" or "EXTRACT(YEAR FROM col)"
pub fn apply_date_transform(name: &str, column: &str, backend: Backend) -> Option<String> {
    let sql = match (name, backend) {
        ("date", _) => format!("DATE({})", column),

        ("year", Backend::PostgreSQL) => format!("EXTRACT(YEAR FROM {})", column),
        ("year", Backend::MySQL) => format!("YEAR({})", column),
        ("year", Backend::SQLite) => format!("CAST(strftime('%Y', {}) AS TEXT)", column),

        ("month", Backend::PostgreSQL) => format!("EXTRACT(MONTH FROM {})", column),
        ("month", Backend::MySQL) => format!("MONTH({})", column),
        ("month", Backend::SQLite) => format!("CAST(strftime('%m', {}) AS TEXT)", column),

        ("day", Backend::PostgreSQL) => format!("EXTRACT(DAY FROM {})", column),
        ("day", Backend::MySQL) => format!("DAYOFMONTH({})", column),
        ("day", Backend::SQLite) => format!("CAST(strftime('%d', {}) AS TEXT)", column),

        ("hour", Backend::PostgreSQL) => format!("EXTRACT(HOUR FROM {})", column),
        ("hour", Backend::MySQL) => format!("HOUR({})", column),
        ("hour", Backend::SQLite) => format!("CAST(strftime('%H', {}) AS TEXT)", column),

        ("minute", Backend::PostgreSQL) => format!("EXTRACT(MINUTE FROM {})", column),
        ("minute", Backend::MySQL) => format!("MINUTE({})", column),
        ("minute", Backend::SQLite) => format!("CAST(strftime('%M', {}) AS TEXT)", column),

        ("second", Backend::PostgreSQL) => format!("EXTRACT(SECOND FROM {})", column),
        ("second", Backend::MySQL) => format!("SECOND({})", column),
        ("second", Backend::SQLite) => format!("CAST(strftime('%S', {}) AS TEXT)", column),

        ("week", Backend::PostgreSQL) => format!("EXTRACT(WEEK FROM {})", column),
        ("week", Backend::MySQL) => format!("WEEK({})", column),
        ("week", Backend::SQLite) => format!("CAST(strftime('%W', {}) AS TEXT)", column),

        ("dow", Backend::PostgreSQL) => format!("EXTRACT(DOW FROM {})", column),
        ("dow", Backend::MySQL) => format!("DAYOFWEEK({})", column),
        ("dow", Backend::SQLite) => format!("CAST(strftime('%w', {}) AS TEXT)", column),

        ("quarter", Backend::PostgreSQL) => format!("EXTRACT(QUARTER FROM {})", column),
        ("quarter", Backend::MySQL) => format!("QUARTER({})", column),
        ("quarter", Backend::SQLite) => format!(
            "CAST((CAST(strftime('%m', {}) AS INTEGER) + 2) / 3 AS TEXT)",
            column
        ),

        ("time", Backend::PostgreSQL) => format!("TIME({})", column),
        ("time", Backend::MySQL) => format!("TIME({})", column),
        ("time", Backend::SQLite) => format!("time({})", column),

        ("iso_week", Backend::PostgreSQL) => format!("EXTRACT(ISOWEEK FROM {})", column),
        ("iso_week", Backend::MySQL) => format!(
            "WEEK({}, 1) - WEEK(DATE_SUB({}, INTERVAL (DAYOFWEEK({}) - 1) DAY), 0) + 1",
            column, column, column
        ),
        ("iso_week", Backend::SQLite) => format!("CAST(strftime('%W', {}) AS TEXT)", column),

        ("iso_dow", Backend::PostgreSQL) => format!("EXTRACT(ISODOW FROM {})", column),
        ("iso_dow", Backend::MySQL) => format!("((DAYOFWEEK({}) + 5) % 7) + 1", column),
        ("iso_dow", Backend::SQLite) => format!("CAST(strftime('%w', {}) AS TEXT)", column),

        _ => return None,
    };
    Some(sql)
}

/// `field__date` → `DATE(field)` (backend-aware) - implicit equality
pub fn date_transform(ctx: &LookupContext) -> String {
    match ctx.backend {
        Backend::PostgreSQL => format!("DATE({}) = ?", ctx.column),
        Backend::MySQL => format!("DATE({}) = ?", ctx.column),
        Backend::SQLite => format!("date({}) = CAST(? AS TEXT)", ctx.column),
    }
}

/// `field__year` → `EXTRACT(YEAR FROM field)` or `YEAR(field)` (backend-aware) - implicit equality
pub fn year_transform(ctx: &LookupContext) -> String {
    match ctx.backend {
        Backend::PostgreSQL => format!("EXTRACT(YEAR FROM {}) = ?", ctx.column),
        Backend::MySQL => format!("YEAR({}) = ?", ctx.column),
        Backend::SQLite => format!("CAST(strftime('%Y', {}) AS INTEGER) = ?", ctx.column),
    }
}

/// `field__month` → `EXTRACT(MONTH FROM field)` or `MONTH(field)` (backend-aware) - implicit equality
pub fn month_transform(ctx: &LookupContext) -> String {
    match ctx.backend {
        Backend::PostgreSQL => format!("EXTRACT(MONTH FROM {}) = ?", ctx.column),
        Backend::MySQL => format!("MONTH({}) = ?", ctx.column),
        Backend::SQLite => format!("CAST(strftime('%m', {}) AS INTEGER) = ?", ctx.column),
    }
}

/// `field__day` → `EXTRACT(DAY FROM field)` or `DAY(field)` (backend-aware) - implicit equality
pub fn day_transform(ctx: &LookupContext) -> String {
    match ctx.backend {
        Backend::PostgreSQL => format!("EXTRACT(DAY FROM {}) = ?", ctx.column),
        Backend::MySQL => format!("DAYOFMONTH({}) = ?", ctx.column),
        Backend::SQLite => format!("CAST(strftime('%d', {}) AS INTEGER) = ?", ctx.column),
    }
}

/// `field__hour` → `EXTRACT(HOUR FROM field)` or `HOUR(field)` (backend-aware) - implicit equality
pub fn hour_transform(ctx: &LookupContext) -> String {
    match ctx.backend {
        Backend::PostgreSQL => format!("EXTRACT(HOUR FROM {}) = ?", ctx.column),
        Backend::MySQL => format!("HOUR({}) = ?", ctx.column),
        Backend::SQLite => format!("CAST(strftime('%H', {}) AS INTEGER) = ?", ctx.column),
    }
}

/// `field__minute` → `EXTRACT(MINUTE FROM field)` or `MINUTE(field)` (backend-aware) - implicit equality
pub fn minute_transform(ctx: &LookupContext) -> String {
    match ctx.backend {
        Backend::PostgreSQL => format!("EXTRACT(MINUTE FROM {}) = ?", ctx.column),
        Backend::MySQL => format!("MINUTE({}) = ?", ctx.column),
        Backend::SQLite => format!("CAST(strftime('%M', {}) AS INTEGER) = ?", ctx.column),
    }
}

/// `field__second` → `EXTRACT(SECOND FROM field)` or `SECOND(field)` (backend-aware) - implicit equality
pub fn second_transform(ctx: &LookupContext) -> String {
    match ctx.backend {
        Backend::PostgreSQL => format!("EXTRACT(SECOND FROM {}) = ?", ctx.column),
        Backend::MySQL => format!("SECOND({}) = ?", ctx.column),
        Backend::SQLite => format!("CAST(strftime('%S', {}) AS INTEGER) = ?", ctx.column),
    }
}

/// `field__week` → `EXTRACT(WEEK FROM field)` or `WEEK(field)` (backend-aware) - implicit equality
pub fn week_transform(ctx: &LookupContext) -> String {
    match ctx.backend {
        Backend::PostgreSQL => format!("EXTRACT(WEEK FROM {}) = ?", ctx.column),
        Backend::MySQL => format!("WEEK({}) = ?", ctx.column),
        Backend::SQLite => format!("CAST(strftime('%W', {}) AS INTEGER) = ?", ctx.column),
    }
}

/// `field__dow` → `EXTRACT(DOW FROM field)` or `DAYOFWEEK(field)` (backend-aware) - implicit equality
pub fn dow_transform(ctx: &LookupContext) -> String {
    match ctx.backend {
        Backend::PostgreSQL => format!("EXTRACT(DOW FROM {}) = ?", ctx.column),
        Backend::MySQL => format!("DAYOFWEEK({}) = ?", ctx.column),
        Backend::SQLite => format!("CAST(strftime('%w', {}) AS INTEGER) = ?", ctx.column),
    }
}

/// `field__quarter` → `EXTRACT(QUARTER FROM field)` or `QUARTER(field)` (backend-aware) - implicit equality
pub fn quarter_transform(ctx: &LookupContext) -> String {
    match ctx.backend {
        Backend::PostgreSQL => format!("EXTRACT(QUARTER FROM {}) = ?", ctx.column),
        Backend::MySQL => format!("QUARTER({}) = ?", ctx.column),
        Backend::SQLite => format!(
            "((CAST(strftime('%m', {}) AS INTEGER) + 2) / 3) = ?",
            ctx.column
        ),
    }
}

/// `field__time` → `TIME(field)` or equivalent (backend-aware) - implicit equality
pub fn time_transform(ctx: &LookupContext) -> String {
    match ctx.backend {
        Backend::PostgreSQL => format!("TIME({}) = ?", ctx.column),
        Backend::MySQL => format!("TIME({}) = ?", ctx.column),
        Backend::SQLite => format!("time({}) = ?", ctx.column),
    }
}

/// `field__iso_week` → `EXTRACT(ISOWEEK FROM field)` or equivalent (backend-aware) - implicit equality
pub fn iso_week_transform(ctx: &LookupContext) -> String {
    match ctx.backend {
        Backend::PostgreSQL => format!("EXTRACT(ISOWEEK FROM {}) = ?", ctx.column),
        Backend::MySQL => format!(
            "WEEK({}, 1) - WEEK(DATE_SUB({}, INTERVAL (DAYOFWEEK({}) - 1) DAY), 0) + 1 = ?",
            ctx.column, ctx.column, ctx.column
        ),
        Backend::SQLite => format!("CAST(strftime('%W', {}) AS INTEGER) = ?", ctx.column),
    }
}

/// `field__iso_dow` → `EXTRACT(ISODOW FROM field)` or equivalent (backend-aware) - implicit equality
pub fn iso_dow_transform(ctx: &LookupContext) -> String {
    match ctx.backend {
        Backend::PostgreSQL => format!("EXTRACT(ISODOW FROM {}) = ?", ctx.column),
        Backend::MySQL => format!("((DAYOFWEEK({}) + 5) % 7) + 1 = ?", ctx.column),
        Backend::SQLite => format!("CAST(strftime('%w', {}) AS INTEGER) = ?", ctx.column),
    }
}
