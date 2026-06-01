use ryx_common::RyxResult;
pub use ryx_backend::backends::RowView;

/// A row decoded from the database.
/// Provides typed access to column values.
pub trait FromRow: Sized {
    fn from_row(row: &RowView) -> RyxResult<Self>;
}
