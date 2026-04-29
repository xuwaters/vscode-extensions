//! Embedded fish-derived completions database.
//!
//! Pairs with the runtime crate [`cli_completions`]. The blob in this
//! crate is generated at build time by `build.rs` from the vendored
//! fish-shell snapshot under `data/fish-snapshot/`; this file just
//! `include_bytes!`s it and exposes a lazy decoder.
//!
//! The [`extractor`] module (the build-time pipeline) is also exposed
//! so consumers can run the same lexer/parser against arbitrary `.fish`
//! input — useful for tests and for tools that want to fold in a
//! per-workspace override directory (RFC 005 §14).
//!
//! # Example
//!
//! ```no_run
//! use cli_completions_data_fish as fish;
//!
//! let db = fish::embedded();
//! for m in db.query(&["curl"], "--an") {
//!     println!("{}\t{}", m.label, m.description.unwrap_or(""));
//! }
//! ```
//!
//! # License
//!
//! GPL-2.0-or-later — see `LICENSE` and RFC 005 §11.

use std::sync::OnceLock;

use cli_completions::CompletionsDb;

pub mod extractor;

/// The packed blob, embedded into the crate at compile time.
///
/// Available as a public constant for callers that want to feed the
/// raw bytes to a custom decoder, write them to disk, or hash them.
pub static BLOB: &[u8] = include_bytes!("../data/embed/completions.bin");

/// Borrow the lazily-decoded database. The first call validates the
/// header; subsequent calls are `O(1)` reads of the cached view.
///
/// Panics if the embedded blob fails the format check, which would
/// indicate a build-script bug rather than a runtime concern.
pub fn embedded() -> &'static CompletionsDb<'static> {
    static DB: OnceLock<CompletionsDb<'static>> = OnceLock::new();
    DB.get_or_init(|| {
        CompletionsDb::from_bytes(BLOB).expect("baked-in blob must be valid")
    })
}
