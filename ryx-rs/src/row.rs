use ryx_common::RyxResult;
pub use ryx_backend::backends::RowView;

/// A row decoded from the database.
/// Provides typed access to column values.
pub trait FromRow: Sized {
    fn from_row(row: &RowView) -> RyxResult<Self>;

    /// Deserialize from a JOIN result row with prefixed relation columns.
    /// Default implementation calls `from_row` (no relation fields).
    fn from_row_joined(row: &RowView) -> RyxResult<Self> {
        Self::from_row(row)
    }

    /// Deserialize from columns prefixed with `{prefix}__`.
    /// Default implementation calls `from_row` (ignores prefix).
    fn from_row_prefixed(row: &RowView, _prefix: &str) -> RyxResult<Self> {
        Self::from_row(row)
    }
}
