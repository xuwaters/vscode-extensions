//! The only crate in the stack that knows WASM exists.
//!
//! `wasm-bindgen` and `js-sys` are quarantined here; everything below — the
//! syntax layer, the naga bridge, every LSP feature — is plain Rust that
//! `cargo test` exercises directly.
//!
//! Built for `wasm32-unknown-unknown` and loaded from a Node child process:
//!
//! ```js
//! const { ShaderServer } = require('./wasm/wgsl_lsp_wasm.js');
//! const server = new ShaderServer(initOptions);
//! const result = server.onRequest('textDocument/hover', params);
//! for (const event of server.drainEvents()) connection.sendNotification(event.method, event.params);
//! ```
//!
//! Unlike the typst server next door, this one takes no host callbacks: it has
//! no filesystem to read, no fonts to load and no packages to resolve.
//! Documents arrive over `didOpen`/`didChange` and workspace files over
//! `wgsl/workspaceFiles`, which is the whole of its input.
//!
//! On a native target this crate compiles to almost nothing on purpose: the
//! bindings are gated on `target_arch = "wasm32"` so `cargo test` works with no
//! WASM toolchain installed.

#[cfg(target_arch = "wasm32")]
mod server;

#[cfg(target_arch = "wasm32")]
pub use server::{InitOptions, ShaderServer};

/// The naga version the server validates against.
pub const NAGA_VERSION: &str = wgsl_lsp_core::NAGA_VERSION;
