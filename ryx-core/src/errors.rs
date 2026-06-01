/// Re-export the core error types from ryx-common (no PyO3 dependency).
///
/// The `From<RyxError> for PyErr` conversion lives in `ryx-python`
/// where both `ryx-common` and `pyo3` are available as direct deps.
pub use ryx_common::errors::{RyxError, RyxResult};
