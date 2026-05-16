//! dotenv-analyzer — a Rust analyzer for `.env` files.
//!
//! The editor-facing surface is [`wasm_api`], which marshals `JSON in,
//! JSON out` requests into the [`vfs`] workspace. Parsing is a single
//! pass that produces an [`ast::File`] of entries (assignments, comments,
//! blank lines) plus a list of [`diagnostics::DotenvDiagnostic`]s.

pub mod ast;
pub mod diagnostics;
pub mod features;
pub mod parse;
pub mod parser;
pub mod spans;
pub mod vfs;
pub mod wasm_api;
