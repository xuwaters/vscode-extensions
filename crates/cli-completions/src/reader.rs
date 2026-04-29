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
    #[allow(dead_code)] // Bounds-tracked for header validation only.
    path_idx_count: usize,
    args_idx_off: usize,
    #[allow(dead_code)] // Bounds-tracked for header validation only.
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
    /// The iterator's mode is selected by `prefix`:
    /// - leading `-` → option matches (long / short flags).
    /// - non-empty, non-`-` → subcommand-name matches.
    /// - empty → subcommand matches first, then option matches.
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
        let query_subpath: &'a [&'a str] = if path.is_empty() { &[] } else { &path[1..] };

        let do_options = prefix.starts_with('-') || prefix.is_empty();
        let do_subcommands = !prefix.starts_with('-');

        let queued = if do_subcommands {
            self.collect_subcommands(entries_idx, entries_count, query_subpath, prefix)
        } else {
            Vec::new()
        };

        CompletionIter {
            db: self,
            queued,
            queued_index: 0,
            next_entry: if do_options { entries_idx } else { entries_idx + entries_count },
            end_entry: entries_idx + entries_count,
            query_subpath,
            prefix,
        }
    }

    /// Iterate over `arg_values` registered for `option_label` under
    /// `path`. `option_label` is the user-visible flag form (e.g.
    /// `"--format"` or `"-f"`); `value_prefix` is whatever the user has
    /// typed after the `=` sign so far.
    ///
    /// Use this to surface enum-style values: `tar --format=g` →
    /// `gnu`, `--gnu`-derivatives, etc.
    pub fn query_arg_values<'a>(
        &'a self,
        path: &'a [&'a str],
        option_label: &str,
        value_prefix: &str,
    ) -> Vec<CompletionMatch<'data>> {
        let Some(cmd_idx) = path.first().and_then(|c| self.find_command(c)) else {
            return Vec::new();
        };
        let cmd = self.read_command(cmd_idx);
        let query_subpath: &[&str] = if path.is_empty() { &[] } else { &path[1..] };

        let mut out: Vec<CompletionMatch<'data>> = Vec::new();
        let mut seen: Vec<u32> = Vec::new();

        for i in 0..cmd.entries_count {
            let entry = self.read_entry(cmd.entries_idx + i);
            if !self.read_path(&entry).applies_to(query_subpath) {
                continue;
            }
            if !self.entry_label_matches(&entry, option_label) {
                continue;
            }
            if entry.args_count == 0 {
                continue;
            }
            for j in 0..entry.args_count {
                let off = self.read_args_idx_slot(entry.args_idx + j);
                if seen.contains(&off) {
                    continue;
                }
                let label = self.read_pool_b(off);
                if !crate::matcher::matches_prefix(label, value_prefix) {
                    continue;
                }
                seen.push(off);
                out.push(CompletionMatch {
                    kind: MatchKind::ArgValue,
                    label: Cow::Borrowed(label),
                    description: None,
                    flags: EntryFlags::from_bits(entry.flags),
                });
            }
        }
        out.sort_by(|a, b| a.label.cmp(&b.label));
        out
    }

    fn entry_label_matches(&self, entry: &EntryRec, option_label: &str) -> bool {
        if let Some(rest) = option_label.strip_prefix("--") {
            if entry.long_off != 0 {
                let long = self.read_pool_b(entry.long_off);
                return long.strip_prefix("--").map_or(long == rest, |s| s == rest);
            }
            return false;
        }
        if let Some(rest) = option_label.strip_prefix('-') {
            if rest.len() == 1 && entry.short != 0 {
                return entry.short == rest.as_bytes()[0];
            }
        }
        false
    }

    fn collect_subcommands(
        &self,
        entries_idx: u32,
        entries_count: u32,
        query_subpath: &[&str],
        prefix: &str,
    ) -> Vec<CompletionMatch<'data>> {
        let mut out: Vec<CompletionMatch<'data>> = Vec::new();
        let mut seen: Vec<u32> = Vec::new();

        for i in 0..entries_count {
            let entry = self.read_entry(entries_idx + i);
            let entry_path_len = entry.path_len as usize;
            if entry_path_len <= query_subpath.len() {
                continue;
            }
            // Verify the entry's path prefix matches `query_subpath`.
            let mut ok = true;
            for j in 0..query_subpath.len() {
                let off = self.read_path_idx_slot(entry.path_idx + j as u32);
                if self.read_pool_b(off) != query_subpath[j] {
                    ok = false;
                    break;
                }
            }
            if !ok {
                continue;
            }
            let cand_off = self.read_path_idx_slot(entry.path_idx + query_subpath.len() as u32);
            if seen.contains(&cand_off) {
                continue;
            }
            let label = self.read_pool_b(cand_off);
            if !crate::matcher::matches_prefix(label, prefix) {
                continue;
            }
            seen.push(cand_off);
            out.push(CompletionMatch {
                kind: MatchKind::Subcommand,
                label: Cow::Borrowed(label),
                description: None,
                flags: EntryFlags::default(),
            });
        }
        out.sort_by(|a, b| a.label.cmp(&b.label));
        out
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

    fn read_args_idx_slot(&self, slot: u32) -> u32 {
        let off = self.args_idx_off + (slot as usize) * 4;
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
    args_idx: u32,
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
///
/// Drains a small queue of pre-collected subcommand matches first
/// (deduplicated and prefix-filtered at construction time), then walks
/// entries lazily for option matches. Construction-time work is
/// `O(N_entries)` only when the prefix admits subcommands; the option
/// walk remains `O(N_entries)` lazily across calls.
pub struct CompletionIter<'a, 'data> {
    db: &'a CompletionsDb<'data>,
    queued: Vec<CompletionMatch<'data>>,
    queued_index: usize,
    next_entry: u32,
    end_entry: u32,
    query_subpath: &'a [&'a str],
    prefix: &'a str,
}

impl<'a, 'data: 'a> Iterator for CompletionIter<'a, 'data> {
    type Item = CompletionMatch<'data>;

    fn next(&mut self) -> Option<Self::Item> {
        // Drain queued subcommand matches first (constructed at query()
        // time and pre-sorted alphabetically).
        if self.queued_index < self.queued.len() {
            let i = self.queued_index;
            self.queued_index += 1;
            return Some(self.queued[i].clone());
        }

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

            let description = if entry.desc_off == 0 {
                None
            } else {
                Some(self.db.read_pool_c(entry.desc_off))
            };

            // Try long first, then short. We emit the first label that
            // matches the prefix; in practice an entry rarely has both
            // forms matching the same prefix (e.g. prefix "--" excludes
            // shorts entirely), so the simple one-shot model is fine.
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
