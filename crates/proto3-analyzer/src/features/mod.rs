//! LSP-style feature providers — phase-1 focus on document symbols and
//! workspace symbols. The other providers (completion, hover, definition,
//! references, rename, ...) are stubbed to return empty results so the
//! WASM boundary is stable.

pub mod document_symbols;
pub mod workspace_symbols;
