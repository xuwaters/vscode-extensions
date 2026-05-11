//! Transport-agnostic log engine.
//!
//! The engine owns log line semantics: ANSI rendering, filter matching, line
//! indexing, and the on-disk index file format. It does **no I/O**: callers
//! hand it byte slices and receive results back, which is what lets the same
//! code power the WASM extension, a CLI, or a future out-of-process server.

pub mod ansi;
pub mod filter;
pub mod index;
pub mod render;

pub use ansi::{Line, parse_lines, strip_ansi};
pub use filter::{Matcher, Rule, build, build_search, compile};
pub use index::{
    INDEX_MAGIC, INDEX_VERSION, IndexHeader, decode_header, encode_header,
    find_newlines, find_newlines_into,
};
pub use render::{LinesPayload, match_lines, render_lines, render_lines_json, search_lines};
