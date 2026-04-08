//
// ###
// Ryx — Common Lookups
// ###
//
// Contains comparison and string lookups (exact, gt, contains, etc.)
// ###

use crate::lookups::LookupContext;

pub use crate::lookups::LookupFn;
pub use crate::lookups::PythonLookup;

/// `field__exact=value` → `field = ?`
///
/// This is also the *implicit* lookup: `filter(name="Alice")` is equivalent
/// to `filter(name__exact="Alice")`.
pub fn exact(ctx: &LookupContext) -> String {
    format!("{} = ?", ctx.column)
}

/// `field__gt=value` → `field > ?`
pub fn gt(ctx: &LookupContext) -> String {
    format!("{} > ?", ctx.column)
}

/// `field__gte=value` → `field >= ?`
pub fn gte(ctx: &LookupContext) -> String {
    format!("{} >= ?", ctx.column)
}

/// `field__lt=value` → `field < ?`
pub fn lt(ctx: &LookupContext) -> String {
    format!("{} < ?", ctx.column)
}

/// `field__lte=value` → `field <= ?`
pub fn lte(ctx: &LookupContext) -> String {
    format!("{} <= ?", ctx.column)
}

/// `field__contains="bob"` → `field LIKE ?`  (with `%value%` at bind time)
///
/// Case-sensitive substring match. The `%` wrapping is applied by the
/// executor when binding the value, not in the SQL fragment itself.
pub fn contains(ctx: &LookupContext) -> String {
    format!("{} LIKE ?", ctx.column)
}

/// `field__icontains="bob"` → `LOWER(field) LIKE LOWER(?)`
///
/// Case-insensitive substring match. Works on all backends without relying
/// on PostgreSQL-specific `ILIKE`. The `%value%` wrapping happens at bind time.
pub fn icontains(ctx: &LookupContext) -> String {
    format!("LOWER({}) LIKE LOWER(?)", ctx.column)
}

/// `field__startswith="pr"` → `field LIKE ?`  (with `value%` at bind time)
pub fn startswith(ctx: &LookupContext) -> String {
    format!("{} LIKE ?", ctx.column)
}

/// `field__istartswith="pr"` → `LOWER(field) LIKE LOWER(?)`
pub fn istartswith(ctx: &LookupContext) -> String {
    format!("LOWER({}) LIKE LOWER(?)", ctx.column)
}

/// `field__endswith="ing"` → `field LIKE ?`  (with `%value` at bind time)
pub fn endswith(ctx: &LookupContext) -> String {
    format!("{} LIKE ?", ctx.column)
}

/// `field__iendswith="ing"` → `LOWER(field) LIKE LOWER(?)`
pub fn iendswith(ctx: &LookupContext) -> String {
    format!("LOWER({}) LIKE LOWER(?)", ctx.column)
}

/// `field__isnull=True` → `field IS NULL`
/// `field__isnull=False` → `field IS NOT NULL`
///
/// Note: the True/False distinction is handled by the compiler which reads the
/// bound value. This function always returns the IS NULL form; the compiler
/// swaps to IS NOT NULL when the value is False/0.
pub fn isnull(ctx: &LookupContext) -> String {
    format!("{} IS NULL", ctx.column)
}

/// `field__in=[1, 2, 3]` → `field IN (?, ?, ?)`
///
/// Note: this returns a *template* — the compiler replaces `(?)` with
/// the correct number of placeholders based on the list length.
pub fn in_lookup(ctx: &LookupContext) -> String {
    format!("{} IN (?)", ctx.column)
}

/// `field__range=(low, high)` → `field BETWEEN ? AND ?`
///
/// Uses two bind parameters. The compiler handles this specially.
pub fn range(ctx: &LookupContext) -> String {
    format!("{} BETWEEN ? AND ?", ctx.column)
}
