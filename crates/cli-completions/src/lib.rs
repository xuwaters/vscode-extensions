//! Runtime decoder and packed blob format for command-line completion data.
//!
//! `cli-completions` is the MIT-licensed half of the system described in
//! RFC 005. It defines a compact little-endian binary format for storing
//! a database of command-line options (flags, subcommands, static argument
//! values) keyed by command + subcommand path, plus a zero-copy decoder
//! that answers `query(path, prefix) -> Iter<CompletionMatch>` against
//! such a blob.
//!
//! The crate has no runtime dependencies and performs no I/O. A sibling
//! crate, `cli-completions-data-fish`, ships a blob produced from a
//! vendored snapshot of fish-shell's completion files.
//!
//! # Quick example
//!
//! ```
//! use cli_completions::{Builder, CompletionsDb, DirectiveInput, EntryFlags};
//!
//! let mut b = Builder::new();
//! b.add(DirectiveInput {
//!     command: "curl",
//!     short: None,
//!     long: Some("anyauth"),
//!     description: Some("(HTTP) Use most secure authentication method automatically"),
//!     flags: EntryFlags::default(),
//!     subcommand_path: &[],
//!     arg_values: &[],
//! });
//! let blob = b.build();
//!
//! let db = CompletionsDb::from_bytes(&blob).unwrap();
//! let matches: Vec<_> = db.query(&["curl"], "--an").collect();
//! assert_eq!(matches.len(), 1);
//! assert_eq!(matches[0].label, "--anyauth");
//! ```

pub mod format;
pub mod matcher;
pub mod reader;
pub mod types;
pub mod writer;

pub use format::{MAGIC, VERSION};
pub use reader::{CompletionIter, CompletionsDb, DumpCommand, DumpEntry};
pub use types::{CompletionMatch, EntryFlags, FormatError, MatchKind};
pub use writer::{Builder, DirectiveInput};
