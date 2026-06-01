//! Feature providers — each submodule answers one LSP-ish query.

pub mod completion;
pub mod definition;
pub mod hover;
pub mod position;
pub mod symbols;
pub mod workspace_symbols;

pub use symbols::{document_symbols, folding_ranges, FoldingRange, Symbol, SymbolKind};
