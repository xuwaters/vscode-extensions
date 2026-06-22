//! cargo-make-analyzer — a Rust analyzer for `Makefile.toml` files.
//!
//! The editor-facing surface is the [`wasm_api`] module, which marshals
//! `JSON in, JSON out` requests into the [`vfs`] workspace. Parsing reuses
//! the lenient [`toml_edit`] document model in [`parse`]: the TOML document
//! is mapped into a small typed [`ast`] (tasks, env vars, config keys) with
//! byte spans, and structural lints are collected in [`diagnostics`].
//!
//! Feature providers in [`features`] are pure views over the AST. The
//! schema of known cargo-make keys (task fields, config keys, condition
//! keys, script runners) lives in [`schema`] and powers completion, hover,
//! and the "unknown key" lints.

pub mod ast;
pub mod diagnostics;
pub mod features;
pub mod parse;
pub mod schema;
pub mod spans;
pub mod vfs;
pub mod wasm_api;
