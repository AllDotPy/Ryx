//
// ###
// Ryx — Compiler Helpers
// ###
//
// Contains internal helper functions for SQL compilation:
// - Identifier quoting (quote_col, qualified_col, split_qualified)
// - LIKE wrapping (apply_like_wrapping)
// - Other compilation utilities
// ###

use crate::ast::SqlValue;

/// Double-quote a simple identifier (column or table name).
pub fn quote_col(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// Handle `table.column` → `"table"."column"`, or plain column → `"column"`.
/// Also handles annotation aliases (already an expression — left as-is).
pub fn qualified_col(s: &str) -> String {
    if s.contains('.') {
        let (table, col) = s.split_once('.').unwrap();
        format!("{}.{}", quote_col(table), quote_col(col))
    } else {
        quote_col(s)
    }
}

/// Split `"table.column"` into `("table", "column")`.
/// Returns `("", s)` if there is no dot.
pub fn split_qualified(s: &str) -> (String, String) {
    if let Some((t, c)) = s.split_once('.') {
        (t.to_string(), c.to_string())
    } else {
        (String::new(), s.to_string())
    }
}

/// Apply LIKE `%` wrapping to the value based on the lookup type.
pub fn apply_like_wrapping(lookup: &str, value: SqlValue) -> SqlValue {
    match lookup {
        "contains" | "icontains" => wrap_text(value, |s| format!("%{s}%")),
        "startswith" | "istartswith" => wrap_text(value, |s| format!("{s}%")),
        "endswith" | "iendswith" => wrap_text(value, |s| format!("%{s}")),
        _ => value,
    }
}

fn wrap_text(value: SqlValue, f: impl Fn(String) -> String) -> SqlValue {
    if let SqlValue::Text(s) = value {
        SqlValue::Text(f(s))
    } else {
        value
    }
}

/// Known transforms that can be applied in field paths
pub const KNOWN_TRANSFORMS: [&str; 16] = [
    "date", "year", "month", "day", "hour", "minute", "second", "week", "dow", "quarter", "time",
    "iso_week", "iso_dow", "key", "key_text", "json",
];
