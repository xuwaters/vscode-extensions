//! mojom-analyzer — a Rust analyzer for Mojom interface definition files
//! (`.mojom`), the IDL used by Chromium's Mojo IPC system.
//!
//! The crate follows the same shape as its sibling IDL analyzers in this
//! workspace: lex → parse → diagnose → resolve → answer LSP-ish queries,
//! all compiled to WebAssembly so the VSCode host needs no native binary.
//!
//! Modules:
//! - [`spans`]  — byte spans and line/col conversions (re-exported from
//!   `analyzer-core`; columns are UTF-16, matching the LSP spec).
//! - [`lexer`]  — hand-written tokenizer producing a flat `Vec<Token>` with
//!   leading comment trivia attached for doc-comment extraction.
//! - [`ast`]    — typed AST nodes; every node carries a [`spans::ByteSpan`].
//! - [`parser`] — recursive-descent parser, recovers at `;` / `}`.
//! - [`diagnostics`] — parse + structural checks plus workspace-level
//!   unresolved-import / unknown-type checks.
//! - [`resolve`] — module-qualified workspace symbol index.
//! - [`features`]    — document symbols, folding, hover, definition,
//!   completion and workspace symbols.
//! - [`vfs`]         — uri → parsed file workspace with import resolution.
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
