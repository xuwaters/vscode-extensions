//! RON (Rusty Object Notation) analyzer.
//!
//! One tolerant parser produces a lossless AST: scalars keep their source
//! span instead of a decoded value, and comments attach to the entry they
//! annotate. That is what lets the formatter reprint a file without
//! re-spelling a number, normalizing an escape, or dropping a comment.
//!
//! Everything inside the crate is byte offsets; [`wasm_api`] converts to
//! UTF-16 line/column on the way out and is the only module that knows
//! WASM exists.

pub mod ast;
pub mod diagnostics;
pub mod extensions;
pub mod features;
pub mod lexer;
pub mod parser;
pub mod spans;
pub mod wasm_api;
pub mod workspace;
