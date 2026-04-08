//
// ###
// Ryx — Lookup Module
// ###
//
// This module provides the lookup system - the suffix after `__` in filter expressions.
// Examples:
//   `age__gte=25`           → lookup = "gte",  SQL = "age >= $1"
//   `name__icontains="bob"` → lookup = "icontains", SQL = "LOWER(name) LIKE LOWER($1)"
//
// The module is organized as:
//   - mod.rs          : Re-exports from lookups.rs
//   - lookups.rs     : Core types, registry, resolve() logic
//   - common_lookups.rs: Comparison and string lookups (exact, gt, contains, etc.)
//   - date_lookups.rs : Date/time transforms (year, month, day, etc.)
//   - json_lookups.rs : JSON transforms and lookups (key, has_key, etc.)
// ###

pub mod common_lookups;
pub mod date_lookups;
pub mod json_lookups;
pub mod lookups;

// Re-export main types from lookups.rs
pub use lookups::LookupContext;
pub use lookups::LookupFn;
pub use lookups::PythonLookup;

// Re-export functions from lookups.rs
pub use lookups::apply_transform;
pub use lookups::init_registry;
pub use lookups::register_custom;
pub use lookups::registered_lookups;
pub use lookups::resolve;
pub use lookups::all_lookups;
pub use lookups::all_transforms;
