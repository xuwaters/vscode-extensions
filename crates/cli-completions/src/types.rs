//! Public types: errors, match kinds, entry flags, the match record.

use std::borrow::Cow;
use std::fmt;

/// Reasons a blob may fail to decode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatError {
    /// Blob is shorter than the fixed header.
    TooShort,
    /// Magic bytes do not match `MAGIC`.
    BadMagic,
    /// Version field is not understood by this crate.
    UnsupportedVersion(u32),
    /// A header offset/length points outside the blob.
    OutOfBounds {
        /// Human label for which field tripped the check.
        field: &'static str,
    },
    /// A string offset is not preceded by a NUL terminator.
    BadString,
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort => f.write_str("blob shorter than header"),
            Self::BadMagic => f.write_str("bad magic"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported version: {v}"),
            Self::OutOfBounds { field } => write!(f, "field {field} points outside blob"),
            Self::BadString => f.write_str("string is not NUL-terminated within its pool"),
        }
    }
}

impl std::error::Error for FormatError {}

/// Classifies what a `CompletionMatch` represents to the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchKind {
    /// A long option such as `--verbose`.
    Long,
    /// A short option such as `-v`.
    Short,
    /// A static argument value (e.g. `--format=` accepts `gnu`, `pax`, …).
    ArgValue,
    /// A subcommand name (e.g. `git remote`'s `add`, `prune`, …).
    Subcommand,
}

/// Bit-flags packed into one byte per entry on disk.
///
/// The flags carry semantic information from fish's `complete` directive
/// (`-x`, `-f`, `-r`, `-F`, plus a couple of internal markers).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EntryFlags(pub u8);

impl EntryFlags {
    /// Option requires an argument (fish `-r`, also implied by `-x`).
    pub const REQUIRES_ARG: u8 = 1 << 0;
    /// Don't fall back to file-name completion (fish `-f`, also implied by `-x`).
    pub const NO_FILES: u8 = 1 << 1;
    /// Force file completion even after this option (fish `-F`).
    pub const FORCE_FILES: u8 = 1 << 2;
    /// Entry has a non-empty static `arg_values` list.
    pub const HAS_ARG_VALUES: u8 = 1 << 3;
    /// Entry only matches at the top level (`__fish_use_subcommand`,
    /// `__fish_<cmd>_needs_command`). Suppresses the entry once any
    /// subcommand has been entered.
    pub const TOP_LEVEL_ONLY: u8 = 1 << 4;

    /// Construct from the raw byte.
    pub const fn from_bits(b: u8) -> Self {
        Self(b)
    }

    /// Test a flag bit.
    pub const fn contains(self, bit: u8) -> bool {
        self.0 & bit != 0
    }

    /// Set a flag bit.
    pub fn insert(&mut self, bit: u8) {
        self.0 |= bit;
    }
}

/// One match returned by `CompletionsDb::query`.
///
/// `description` always borrows from the underlying blob. `label` borrows
/// from the blob for long options, arg values, and subcommands; for
/// short options (`-v`) the iterator constructs a 2-byte `String` since
/// the blob stores only the single byte. Practically that's at most one
/// tiny allocation per short match returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionMatch<'a> {
    /// Kind of completion (long / short / arg value / subcommand).
    pub kind: MatchKind,
    /// User-visible label (e.g. `--anyauth`, `-v`, `gnu`, `add`).
    pub label: Cow<'a, str>,
    /// Optional human description.
    pub description: Option<&'a str>,
    /// Bit-flags for the underlying entry.
    pub flags: EntryFlags,
}
