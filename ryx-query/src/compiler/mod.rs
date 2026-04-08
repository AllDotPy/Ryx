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

pub mod compiler;
pub mod helpers;

// Re-export from compiler.rs
pub use compiler::compile;
pub use compiler::compile_agg_cols;
pub use compiler::compile_joins;
pub use compiler::compile_order_by;
pub use compiler::compile_q;
pub use compiler::CompiledQuery;

// Re-export from helpers.rs
pub use helpers::apply_like_wrapping;
pub use helpers::qualified_col;
pub use helpers::quote_col;
pub use helpers::split_qualified;
pub use helpers::KNOWN_TRANSFORMS;
