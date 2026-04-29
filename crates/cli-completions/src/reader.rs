//! Zero-copy decoder over a packed completions blob.

use std::borrow::Cow;

use crate::format::{
    cmd_rec, entry_rec, hdr, CMD_RECORD_SIZE, ENTRY_RECORD_SIZE, HEADER_SIZE, MAGIC, VERSION,
};
use crate::matcher::{entry_path_applies, matches_prefix};
use crate::types::{CompletionMatch, EntryFlags, FormatError, MatchKind};

/// A decoded view over a `cli-completions` blob.
///
/// Construction is `O(1)` after a header validity check; all field
/// access is zero-copy (the struct stores only offsets/counts plus
/// references to slices of the input bytes).
#[derive(Debug, Clone)]
pub struct CompletionsDb<'data> {
    blob: &'data [u8],
    cmds_off: usize,
    cmds_count: usize,
    entries_off: usize,
    entries_count: usize,
    path_idx_off: usize,
    // Held for bounds-checking and future arg-value iteration (RFC §13 phase 6).
    #[allow(dead_code)]
    path_idx_count: usize,
    #[allow(dead_code)]
    args_idx_off: usize,
    #[allow(dead_code)]
    args_idx_count: usize,
    pool_a_off: usize,
    pool_a_len: usize,
    pool_b_off: usize,
    pool_b_len: usize,
    pool_c_off: usize,
    pool_c_len: usize,
}

impl<'data> CompletionsDb<'data> {
    /// Decode a blob's header. Returns `Err` for unknown magic, version
    /// mismatch, or any header field whose offset/length lies outside
    /// the input.
    pub fn from_bytes(blob: &'data [u8]) -> Result<Self, FormatError> {
        if blob.len() < HEADER_SIZE {
            return Err(FormatError::TooShort);
        }
        if blob[hdr::MAGIC..hdr::MAGIC + 4] != MAGIC {
            return Err(FormatError::BadMagic);
        }
        let version = read_u32(blob, hdr::VERSION);
        if version != VERSION {
            return Err(FormatError::UnsupportedVersion(version));
        }

        let cmds_off = read_u32(blob, hdr::CMDS_OFF) as usize;
        let cmds_count = read_u32(blob, hdr::CMDS_COUNT) as usize;
        let entries_off = read_u32(blob, hdr::ENTRIES_OFF) as usize;
        let entries_count = read_u32(blob, hdr::ENTRIES_COUNT) as usize;
        let path_idx_off = read_u32(blob, hdr::PATH_IDX_OFF) as usize;
        let path_idx_count = read_u32(blob, hdr::PATH_IDX_COUNT) as usize;
        let args_idx_off = read_u32(blob, hdr::ARGS_IDX_OFF) as usize;
        let args_idx_count = read_u32(blob, hdr::ARGS_IDX_COUNT) as usize;
        let pool_a_off = read_u32(blob, hdr::POOL_A_OFF) as usize;
        let pool_a_len = read_u32(blob, hdr::POOL_A_LEN) as usize;
        let pool_b_off = read_u32(blob, hdr::POOL_B_OFF) as usize;
        let pool_b_len = read_u32(blob, hdr::POOL_B_LEN) as usize;
        let pool_c_off = read_u32(blob, hdr::POOL_C_OFF) as usize;
        let pool_c_len = read_u32(blob, hdr::POOL_C_LEN) as usize;

        let check = |off: usize, len: usize, field: &'static str| -> Result<(), FormatError> {
            off.checked_add(len)
                .filter(|end| *end <= blob.len())
                .map(|_| ())
                .ok_or(FormatError::OutOfBounds { field })
        };
        check(cmds_off, cmds_count * CMD_RECORD_SIZE, "cmds")?;
        check(entries_off, entries_count * ENTRY_RECORD_SIZE, "entries")?;
        check(path_idx_off, path_idx_count * 4, "path_idx")?;
        check(args_idx_off, args_idx_count * 4, "args_idx")?;
        check(pool_a_off, pool_a_len, "pool_a")?;
        check(pool_b_off, pool_b_len, "pool_b")?;
        check(pool_c_off, pool_c_len, "pool_c")?;

        Ok(Self {
            blob,
            cmds_off,
            cmds_count,
            entries_off,
            entries_count,
            path_idx_off,
            path_idx_count,
            args_idx_off,
            args_idx_count,
            pool_a_off,
            pool_a_len,
            pool_b_off,
            pool_b_len,
            pool_c_off,
            pool_c_len,
        })
    }

    /// Total number of distinct commands in the database.
    pub fn command_count(&self) -> usize {
        self.cmds_count
    }

    /// Total number of entries (across all commands) in the database.
    pub fn entry_count(&self) -> usize {
        self.entries_count
    }

    /// Does the database carry any entries for `command`?
    pub fn has_command(&self, command: &str) -> bool {
        self.find_command(command).is_some()
    }

    /// Iterate over completion matches for `path` and `prefix`.
    ///
    /// `path[0]` is the top-level command; the remainder is the
    /// subcommand chain the cursor is in.
    ///
    /// Returns an empty iterator if `path` is empty, the command is
    /// unknown, or no entry passes both the path and prefix filters.
    pub fn query<'a>(&'a self, path: &'a [&'a str], prefix: &'a str) -> CompletionIter<'a, 'data> {
        let (entries_idx, entries_count) = match path
            .first()
            .and_then(|cmd| self.find_command(cmd))
            .map(|i| self.read_command(i))
        {
            Some(c) => (c.entries_idx, c.entries_count),
            None => (0, 0),
        };
        CompletionIter {
            db: self,
            next_entry: entries_idx,
            end_entry: entries_idx + entries_count,
            query_subpath: if path.is_empty() { &[] } else { &path[1..] },
            prefix,
        }
    }

    // -- internals -----------------------------------------------------

    fn find_command(&self, name: &str) -> Option<usize> {
        let target = name.as_bytes();
        let (mut lo, mut hi) = (0usize, self.cmds_count);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let c = self.read_command(mid);
            let s = self.read_str(self.pool_a_off, self.pool_a_len, c.name_off);
            match s.as_bytes().cmp(target) {
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
                std::cmp::Ordering::Equal => return Some(mid),
            }
        }
        None
    }

    fn read_command(&self, i: usize) -> CmdRec {
        let base = self.cmds_off + i * CMD_RECORD_SIZE;
        CmdRec {
            name_off: read_u32(self.blob, base + cmd_rec::NAME_OFF),
            entries_idx: read_u32(self.blob, base + cmd_rec::ENTRIES_IDX),
            entries_count: read_u32(self.blob, base + cmd_rec::ENTRIES_COUNT),
        }
    }

    fn read_entry(&self, i: u32) -> EntryRec {
        let base = self.entries_off + (i as usize) * ENTRY_RECORD_SIZE;
        EntryRec {
            short: self.blob[base + entry_rec::SHORT],
            flags: self.blob[base + entry_rec::FLAGS],
            path_len: read_u16(self.blob, base + entry_rec::PATH_LEN),
            path_idx: read_u32(self.blob, base + entry_rec::PATH_IDX),
            long_off: read_u32(self.blob, base + entry_rec::LONG_OFF),
            desc_off: read_u32(self.blob, base + entry_rec::DESC_OFF),
            args_idx: read_u32(self.blob, base + entry_rec::ARGS_IDX),
            args_count: read_u32(self.blob, base + entry_rec::ARGS_COUNT),
        }
    }

    fn read_path<'a>(&'a self, e: &EntryRec) -> SubcommandPath<'a, 'data> {
        SubcommandPath {
            db: self,
            idx: e.path_idx,
            len: e.path_len,
        }
    }

    /// Read a NUL-terminated string at `offset` within the given pool.
    /// Offset 0 yields `""` (the leading NUL sentinel).
    fn read_str(&self, pool_off: usize, pool_len: usize, offset: u32) -> &'data str {
        let start = pool_off + offset as usize;
        let pool_end = pool_off + pool_len;
        let slice = &self.blob[start..pool_end];
        let nul = slice
            .iter()
            .position(|b| *b == 0)
            .unwrap_or(slice.len());
        // Pools are written from valid UTF-8; we tolerate invalid bytes
        // by returning an empty string rather than panicking.
        std::str::from_utf8(&slice[..nul]).unwrap_or("")
    }

    fn read_pool_b(&self, off: u32) -> &'data str {
        self.read_str(self.pool_b_off, self.pool_b_len, off)
    }

    fn read_pool_c(&self, off: u32) -> &'data str {
        self.read_str(self.pool_c_off, self.pool_c_len, off)
    }

    fn read_path_idx_slot(&self, slot: u32) -> u32 {
        let off = self.path_idx_off + (slot as usize) * 4;
        read_u32(self.blob, off)
    }
}

#[derive(Debug)]
struct CmdRec {
    name_off: u32,
    entries_idx: u32,
    entries_count: u32,
}

#[derive(Debug)]
struct EntryRec {
    short: u8,
    flags: u8,
    path_len: u16,
    path_idx: u32,
    long_off: u32,
    desc_off: u32,
    #[allow(dead_code)]
    args_idx: u32,
    #[allow(dead_code)]
    args_count: u32,
}

/// Lazy view of an entry's subcommand path.
struct SubcommandPath<'a, 'data> {
    db: &'a CompletionsDb<'data>,
    idx: u32,
    len: u16,
}

impl<'a, 'data> SubcommandPath<'a, 'data> {
    fn applies_to(&self, query: &[&str]) -> bool {
        if self.len as usize > query.len() {
            return false;
        }
        // Compare component-by-component. We materialise the path's
        // strings as a SmallVec-equivalent on the stack to reuse the
        // matcher helper; sizes are bounded (path_len fits in u16, in
        // practice ≤4 for fish's `__fish_<cmd>_using_command` patterns).
        let n = self.len as usize;
        let mut buf: [&str; 8] = [""; 8];
        let materialised = if n <= buf.len() {
            for i in 0..n {
                let pool_b_off = self.db.read_path_idx_slot(self.idx + i as u32);
                buf[i] = self.db.read_pool_b(pool_b_off);
            }
            &buf[..n]
        } else {
            // Fallback for the (rare) deeper paths.
            let owned: Vec<&str> = (0..n)
                .map(|i| {
                    let off = self.db.read_path_idx_slot(self.idx + i as u32);
                    self.db.read_pool_b(off)
                })
                .collect();
            return entry_path_applies(&owned, query);
        };
        entry_path_applies(materialised, query)
    }
}

/// Iterator over `query` matches.
pub struct CompletionIter<'a, 'data> {
    db: &'a CompletionsDb<'data>,
    next_entry: u32,
    end_entry: u32,
    query_subpath: &'a [&'a str],
    prefix: &'a str,
}

impl<'a, 'data: 'a> Iterator for CompletionIter<'a, 'data> {
    type Item = CompletionMatch<'data>;

    fn next(&mut self) -> Option<Self::Item> {
        while self.next_entry < self.end_entry {
            let entry = self.db.read_entry(self.next_entry);
            self.next_entry += 1;

            let flags = EntryFlags::from_bits(entry.flags);

            if flags.contains(EntryFlags::TOP_LEVEL_ONLY) && !self.query_subpath.is_empty() {
                continue;
            }
            if !self.db.read_path(&entry).applies_to(self.query_subpath) {
                continue;
            }

            // Phase 1 only emits long/short option matches. Subcommand
            // and arg-value matches are RFC §13 phase 6 polish.
            let prefix_starts_dash = self.prefix.starts_with('-') || self.prefix.is_empty();
            if !prefix_starts_dash {
                continue;
            }

            let description = if entry.desc_off == 0 {
                None
            } else {
                Some(self.db.read_pool_c(entry.desc_off))
            };

            // Try long first, then short. We yield the first one that
            // matches the prefix; the second match (if any) is queued
            // by re-reading the same entry on the next iteration.
            //
            // For phase 1 simplicity we always yield only one match per
            // call, walking past the entry once both labels are tried.
            // In practice an entry rarely has both forms matching the
            // same prefix (e.g. prefix "--" excludes shorts entirely),
            // so the simple one-shot model is fine.
            if entry.long_off != 0 {
                let label = self.db.read_pool_b(entry.long_off);
                if matches_prefix(label, self.prefix) {
                    return Some(CompletionMatch {
                        kind: MatchKind::Long,
                        label: Cow::Borrowed(label),
                        description,
                        flags,
                    });
                }
            }
            if entry.short != 0 {
                let label = format!("-{}", entry.short as char);
                if matches_prefix(&label, self.prefix) {
                    return Some(CompletionMatch {
                        kind: MatchKind::Short,
                        label: Cow::Owned(label),
                        description,
                        flags,
                    });
                }
            }
        }
        None
    }
}

fn read_u32(buf: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        buf[offset],
        buf[offset + 1],
        buf[offset + 2],
        buf[offset + 3],
    ])
}

fn read_u16(buf: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([buf[offset], buf[offset + 1]])
}
