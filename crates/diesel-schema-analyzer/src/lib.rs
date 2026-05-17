//! diesel-schema-analyzer — a Rust analyzer for diesel-generated `schema.rs`
//! files.
//!
//! The editor-facing surface is [`wasm_api`], which marshals `JSON in, JSON
//! out` requests into the [`vfs`] workspace. Parsing is a two-step pipeline:
//! a [`lexer`] tokenizes the Rust source (with awareness of comments and
//! strings only — not full Rust syntax), and a [`parser`] picks out the
//! three diesel macro invocations we care about — `diesel::table!`,
//! `diesel::joinable!`, and `diesel::allow_tables_to_appear_in_same_query!`
//! — and builds an [`ast::SchemaFile`].

pub mod ast;
pub mod diagnostics;
pub mod features;
pub mod lexer;
pub mod parse;
pub mod parser;
pub mod spans;
pub mod vfs;
pub mod wasm_api;
