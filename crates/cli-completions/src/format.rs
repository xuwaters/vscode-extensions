//! On-disk format constants for the packed completions blob.
//!
//! All multi-byte fields are little-endian. The blob layout is:
//!
//! ```text
//! +-------------------------------+
//! | Header (FIXED, HEADER_SIZE)   |
//! +-------------------------------+
//! | Command table                 |   header.cmds_count * CMD_RECORD_SIZE
//! +-------------------------------+
//! | Entry table                   |   header.entries_count * ENTRY_RECORD_SIZE
//! +-------------------------------+
//! | Path index (u32 array)        |   pool-B offsets, packed as u32 LE
//! +-------------------------------+
//! | Args index (u32 array)        |   pool-B offsets, packed as u32 LE
//! +-------------------------------+
//! | Pool A (commands)             |   NUL-separated UTF-8, leading NUL
//! +-------------------------------+
//! | Pool B (option / arg names)   |   NUL-separated UTF-8, leading NUL
//! +-------------------------------+
//! | Pool C (descriptions)         |   NUL-separated UTF-8, leading NUL
//! +-------------------------------+
//! ```
//!
//! Each pool begins with a single NUL byte at offset 0, so an offset of
//! 0 unambiguously means "no string" — real strings always live at a
//! nonzero offset.
//!
//! Command records (16 bytes):
//!
//! ```text
//!   name_off       u32   offset into Pool A (>0)
//!   entries_idx    u32   index of first entry in the entry table
//!   entries_count  u32   number of entries belonging to this command
//!   reserved       u32   must be 0
//! ```
//!
//! Entry records (24 bytes):
//!
//! ```text
//!   short        u8     ASCII byte for short option, 0 = none
//!   flags        u8     EntryFlags bitfield
//!   path_len     u16    number of subcommand-path components (LE)
//!   path_idx     u32    index into Path array (in u32 units, not bytes)
//!   long_off     u32    offset into Pool B, 0 = none
//!   desc_off     u32    offset into Pool C, 0 = none
//!   args_idx     u32    index into Args array (in u32 units), 0 = none
//!   args_count   u32    number of arg values
//! ```
//!
//! The command table is sorted by `name_off`'s string content (ascending
//! UTF-8 byte order), enabling binary search.
//!
//! Within a command, entries are sorted by (subcommand path, then label)
//! so the iterator can stop early once it walks past the matching path
//! prefix.

/// Magic bytes identifying a `cli-completions` blob.
pub const MAGIC: [u8; 4] = *b"CLIC";

/// Format version supported by this crate.
pub const VERSION: u32 = 1;

/// Byte size of the fixed header (magic + version + 14 u32 fields).
pub const HEADER_SIZE: usize = 4 + 4 + 14 * 4;

/// Byte size of one command record on disk.
pub const CMD_RECORD_SIZE: usize = 16;

/// Byte size of one entry record on disk.
pub const ENTRY_RECORD_SIZE: usize = 24;

/// Byte offsets of header fields, measured from the start of the blob.
pub(crate) mod hdr {
    pub const MAGIC: usize = 0;
    pub const VERSION: usize = 4;
    pub const CMDS_OFF: usize = 8;
    pub const CMDS_COUNT: usize = 12;
    pub const ENTRIES_OFF: usize = 16;
    pub const ENTRIES_COUNT: usize = 20;
    pub const PATH_IDX_OFF: usize = 24;
    pub const PATH_IDX_COUNT: usize = 28; // count of u32 entries
    pub const ARGS_IDX_OFF: usize = 32;
    pub const ARGS_IDX_COUNT: usize = 36; // count of u32 entries
    pub const POOL_A_OFF: usize = 40;
    pub const POOL_A_LEN: usize = 44;
    pub const POOL_B_OFF: usize = 48;
    pub const POOL_B_LEN: usize = 52;
    pub const POOL_C_OFF: usize = 56;
    pub const POOL_C_LEN: usize = 60;
}

/// Byte offsets of fields within a `CmdRecord`.
pub(crate) mod cmd_rec {
    pub const NAME_OFF: usize = 0;
    pub const ENTRIES_IDX: usize = 4;
    pub const ENTRIES_COUNT: usize = 8;
    pub const RESERVED: usize = 12;
}

/// Byte offsets of fields within an `EntryRecord`.
pub(crate) mod entry_rec {
    pub const SHORT: usize = 0;
    pub const FLAGS: usize = 1;
    pub const PATH_LEN: usize = 2;
    pub const PATH_IDX: usize = 4;
    pub const LONG_OFF: usize = 8;
    pub const DESC_OFF: usize = 12;
    pub const ARGS_IDX: usize = 16;
    pub const ARGS_COUNT: usize = 20;
}
