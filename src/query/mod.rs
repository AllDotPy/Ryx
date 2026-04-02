//
// ###
// Ryx — Query module
//
// This module contains everything related to building and compiling queries:
//   - ast.rs : the query abstract syntax tree (data structures)
//   - lookup.rs : the lookup registry (built-in + user-registered lookups)
//   - compiler.rs : AST → SQL string + bound values
// ###

pub mod ast;
pub mod lookup;
pub mod compiler;