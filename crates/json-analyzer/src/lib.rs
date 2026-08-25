//! JSON family analyzer: JSON, JSONC, JSON5, and JSON Lines.
//!
//! One tolerant JSON5-superset parser serves every flavor; the flavor
//! decides which constructs are *diagnosed*, not which ones parse. The
//! AST is lossless — scalars keep their raw source bytes and comments
//! attach to the member or element they annotate — so the formatter can
//! reprint a file (optionally with object keys sorted recursively)
//! without destroying anything the author wrote.

pub mod ast;
pub mod diagnostics;
pub mod features;
pub mod flavor;
pub mod lexer;
pub mod parser;
pub mod spans;
pub mod wasm_api;
pub mod workspace;
