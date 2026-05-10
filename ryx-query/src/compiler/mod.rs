//
// ###
// Ryx — Compiler Module
// ###
//
// This module contains the SQL compiler that transforms QueryNode AST into SQL strings.
// The module is organized as:
//   - mod.rs      : Re-exports from compiler.rs
//   - compiler.rs: Main implementation (compile, compile_select, etc.)
//   - helpers.rs  : Internal helper functions (quote_col, qualified_col, etc.)
// ###

pub mod compilr;
pub mod helpers;

// Re-export from compiler.rs
pub use compilr::CompiledQuery;
pub use compilr::SqlWriter;
pub use compilr::compile;
pub use compilr::compile_agg_cols;
pub use compilr::compile_joins;
pub use compilr::compile_order_by;
pub use compilr::compile_q;

// Re-export from helpers.rs
pub use helpers::KNOWN_TRANSFORMS;
pub use helpers::apply_like_wrapping;
pub use helpers::qualified_col;
pub use helpers::quote_col;
pub use helpers::split_qualified;
