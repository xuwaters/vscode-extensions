//! Shared building blocks for the per-language analyzer crates.
//!
//! Today this only exposes [`spans`]. Future steps will add a generic
//! diagnostic type, a `Workspace<L>` document store, common LSP value
//! types, and a WASM harness macro — see the refactor plan in the
//! workspace docs.

pub mod spans;
