//! Shared building blocks for the per-language analyzer crates.
//!
//! - [`spans`] — `ByteSpan`, `LineCol`, `SpanTable` (UTF-16 column conversion).
//! - [`diagnostics`] — `Severity`, generic `Diagnostic<C>`, the
//!   `DiagnosticCode` trait.
//! - [`vfs`] — `FileUri`, generic `Workspace<L>`, `ParsedFile<L>`,
//!   `AnalyzerLang` trait.
//!
//! Future steps will add common feature value types (DocumentSymbol,
//! FoldingRange, …) and a WASM harness macro — see the refactor plan.

pub mod diagnostics;
pub mod lsp;
pub mod spans;
pub mod vfs;
