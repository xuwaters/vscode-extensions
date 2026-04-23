//! capnp-analyzer — a Rust analyzer for Cap'n Proto schema files (`.capnp`).
//!
//! The first-cut surface is intentionally focused: lex → parse → diagnose →
//! expose document symbols and folding ranges over WASM. Name resolution,
//! hover, completion, and cross-file imports are deferred to later iterations.
//!
//! Modules:
//! - [`spans`]  — byte spans and line/col conversions (UTF-16 columns for LSP).
//! - [`lexer`]  — hand-written tokenizer, produces a `Vec<Token>` with
//!   leading-trivia attached for later doc-comment extraction.
//! - [`ast`]    — typed AST nodes; every node carries a [`spans::ByteSpan`].
//! - [`parser`] — recursive-descent parser, recovers at `;` / `}`.
//! - [`diagnostics`] — parse + structural checks (duplicate ordinals, etc.).
//! - [`features`]    — document-symbol and folding-range queries.
//! - [`vfs`]         — uri → parsed file workspace.
//! - [`wasm_api`]    — JSON-in / JSON-out handle for the VSCode host.

pub mod ast;
pub mod diagnostics;
pub mod features;
pub mod lexer;
pub mod parser;
pub mod resolve;
pub mod spans;
pub mod vfs;
pub mod wasm_api;
