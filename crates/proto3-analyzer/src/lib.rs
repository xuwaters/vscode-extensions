//! proto3-analyzer — a Rust analyzer for Protocol Buffers (proto3).
//!
//! The crate is organised in three layers:
//!
//! 1. **Parse** — [`lexer`] tokenises the source, [`parser`] turns tokens into
//!    the typed [`ast`] nodes. Every AST node carries a [`spans::ByteSpan`].
//! 2. **Workspace** — [`vfs`] tracks file contents, include paths, and
//!    reverse-import dependencies; [`resolve`] builds a symbol index with
//!    scope-aware name resolution.
//! 3. **Features** — [`diagnostics`], [`features`] expose language-server
//!    style queries (document symbols, completion, hover, etc.). [`wasm_api`]
//!    marshals them over the `wasm-bindgen` boundary.

pub mod ast;
pub mod diagnostics;
pub mod features;
pub mod lexer;
pub mod parse;
pub mod parser;
pub mod resolve;
pub mod spans;
pub mod textproto;
pub mod vfs;
pub mod wasm_api;
pub mod well_known;

#[cfg(feature = "descriptor-adapter")]
pub mod descriptor;
