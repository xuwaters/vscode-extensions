//! RON (Rusty Object Notation) analyzer.
//!
//! One tolerant parser produces a lossless AST: scalars keep their source
//! span instead of a decoded value, and comments attach to the entry they
//! annotate. That is what lets the formatter reprint a file without
//! re-spelling a number, normalizing an escape, or dropping a comment.
//!
//! A RON file may name the Rust type it deserializes into with a comment
//! such as `/* @[crate::config::GameConfig] */`. [`rust`] reads that type's
//! declaration out of the workspace and [`schema`] checks the file against
//! it, which is what powers type diagnostics, field and variant
//! completion, and go-to-definition into the Rust source.
//!
//! Everything inside the crate is byte offsets; the language server
//! (`ron-lsp-core`) converts to LSP positions.

pub mod ast;
pub mod diagnostics;
pub mod extensions;
pub mod features;
pub mod lexer;
pub mod parser;
pub mod rust;
pub mod schema;
pub mod spans;
pub mod workspace;
