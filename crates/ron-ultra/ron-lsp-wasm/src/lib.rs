//! The only crate in the stack that knows WASM exists.
//!
//! Built for `wasm32-unknown-unknown` and loaded from a Node child process:
//!
//! ```js
//! const { RonServer } = require('./wasm/ron_lsp_wasm.js');
//! const server = new RonServer({ readFile: (uri) => text ?? null }, { settings });
//! const result = server.onRequest('textDocument/hover', params);
//! for (const event of server.drainEvents()) connection.sendNotification(event.method, event.params);
//! ```
//!
//! The one host callback, `readFile`, is synchronous: the server calls it
//! in the middle of resolving a `@[crate::...]` annotation to read a
//! `Cargo.toml` or a `.rs` file, and needs the answer before it can go on.
//!
//! On a native target this crate compiles to almost nothing on purpose: the
//! bindings are gated on `target_arch = "wasm32"` so `cargo test` works with
//! no WASM toolchain installed.

#[cfg(target_arch = "wasm32")]
mod server;

#[cfg(target_arch = "wasm32")]
pub use server::RonServer;
