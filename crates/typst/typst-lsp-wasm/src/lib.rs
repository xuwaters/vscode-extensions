//! The only crate in the stack that knows WASM exists.
//!
//! `wasm-bindgen`, `js-sys`, and the `Send`/`Sync` assertion are quarantined
//! here; everything below — the session, the LSP features, the preview — is
//! plain Rust that `cargo test` exercises directly.
//!
//! Built for `wasm32-unknown-unknown` and loaded from a Node child process:
//!
//! ```js
//! const { TypstServer } = require('./wasm/typst_lsp_wasm.js');
//! const server = new TypstServer(hostServices, initOptions);
//! const result = server.onRequest('textDocument/hover', params);
//! for (const event of server.drainEvents()) connection.sendNotification(event.method, event.params);
//! ```
//!
//! On a native target this crate compiles to almost nothing on purpose: the
//! bindings are gated on `target_arch = "wasm32"` so `cargo test` works with no
//! WASM toolchain installed, while [`single_threaded`] still refuses to compile
//! for any *WASM* target that has threads.

pub mod keys;

#[cfg(target_arch = "wasm32")]
mod host;
#[cfg(target_arch = "wasm32")]
mod server;
#[cfg(target_arch = "wasm32")]
mod single_threaded;

#[cfg(target_arch = "wasm32")]
pub use server::{FaceEntry, InitOptions, JsPorts, TypstServer};
