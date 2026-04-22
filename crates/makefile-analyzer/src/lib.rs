//! makefile-analyzer — a Rust analyzer for GNU Makefiles.
//!
//! The crate is organised so the editor-facing surface is the `wasm_api`
//! module, which marshals `JSON in, JSON out` requests into the [`vfs`]
//! workspace. Parsing is a two-stage pipeline: [`lexer`] produces a
//! `Vec<LogicalLine>`, [`parser`] turns those into the typed [`ast`] tree.
//! Feature providers consume the AST in [`features`]; syntactic / structural
//! errors are collected in [`diagnostics`].

pub mod ast;
pub mod diagnostics;
pub mod features;
pub mod lexer;
pub mod parse;
pub mod parser;
pub mod spans;
pub mod vfs;
pub mod wasm_api;
