//! Builder API for producing a packed completions blob.
//!
//! The runtime crate owns the format definition, so it owns both the
//! decoder ([`reader`](super::reader)) and the encoder (this module).
//! `cli-completions-data-fish`'s `build.rs` calls [`Builder`] to emit
//! `data/completions.bin`; tests in this crate use the same API to
//! construct synthetic blobs.

use std::collections::{BTreeMap, HashMap};

use crate::format::{
    cmd_rec, entry_rec, hdr, CMD_RECORD_SIZE, ENTRY_RECORD_SIZE, HEADER_SIZE, MAGIC, VERSION,
};
use crate::types::EntryFlags;

/// One completion directive handed to [`Builder::add`].
///
/// Fields mirror the static subset of fish's `complete` builtin.
#[derive(Debug, Clone, Copy)]
pub struct DirectiveInput<'a> {
    /// Command this directive belongs to (e.g. `"curl"`).
    pub command: &'a str,
    /// Short option byte (e.g. `b'v'`), if any.
    pub short: Option<u8>,
    /// Long option name without the `--` prefix (e.g. `"verbose"`), if any.
    pub long: Option<&'a str>,
    /// Human description, if any.
    pub description: Option<&'a str>,
    /// Bit-flags from fish's `-x` / `-f` / `-r` / `-F`.
    pub flags: EntryFlags,
    /// Subcommand path under which this option applies. Empty = applies
    /// at every depth.
    pub subcommand_path: &'a [&'a str],
    /// Static argument values (fish `-a "X Y Z"`). Empty for none.
    pub arg_values: &'a [&'a str],
}

/// Accumulates directives and produces a packed blob via [`Builder::build`].
///
/// Iteration order is canonicalised (commands sorted by name; entries
/// within a command sorted by `(subcommand_path, primary_label)`), so
/// the output is byte-deterministic for a given input set.
#[derive(Debug, Default)]
pub struct Builder {
    /// Sorted by command name.
    commands: BTreeMap<String, Vec<OwnedDirective>>,
}

#[derive(Debug, Clone)]
struct OwnedDirective {
    short: Option<u8>,
    long: Option<String>,
    description: Option<String>,
    flags: EntryFlags,
    subcommand_path: Vec<String>,
    arg_values: Vec<String>,
}

impl OwnedDirective {
    fn primary_label(&self) -> String {
        if let Some(long) = &self.long {
            format!("--{long}")
        } else if let Some(s) = self.short {
            format!("-{}", s as char)
        } else {
            String::new()
        }
    }

    fn is_useful(&self) -> bool {
        self.long.is_some() || self.short.is_some() || !self.arg_values.is_empty()
    }
}

impl Builder {
    /// Create an empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a directive. Drops it silently if it carries no useful data
    /// (no long, no short, no arg values) — those entries can't surface
    /// anything to the user anyway.
    pub fn add(&mut self, d: DirectiveInput<'_>) {
        let owned = OwnedDirective {
            short: d.short,
            long: d.long.map(str::to_owned),
            description: d.description.map(str::to_owned),
            flags: d.flags,
            subcommand_path: d.subcommand_path.iter().map(|s| (*s).to_owned()).collect(),
            arg_values: d.arg_values.iter().map(|s| (*s).to_owned()).collect(),
        };
        if !owned.is_useful() {
            return;
        }
        self.commands
            .entry(d.command.to_owned())
            .or_default()
            .push(owned);
    }

    /// Encode all accumulated directives into a packed blob.
    pub fn build(self) -> Vec<u8> {
        let mut pool_a = StrPool::new();
        let mut pool_b = StrPool::new();
        let mut pool_c = StrPool::new();
        let mut path_idx: Vec<u32> = Vec::new();
        let mut args_idx: Vec<u32> = Vec::new();

        let mut packed_cmds: Vec<PackedCmd> = Vec::with_capacity(self.commands.len());
        let mut packed_entries: Vec<PackedEntry> = Vec::new();

        for (cmd_name, mut directives) in self.commands {
            let name_off = pool_a.intern(&cmd_name);

            directives.sort_by(|a, b| {
                a.subcommand_path
                    .cmp(&b.subcommand_path)
                    .then_with(|| a.primary_label().cmp(&b.primary_label()))
            });

            let entries_idx = packed_entries.len() as u32;
            for d in directives {
                let path_idx_val = path_idx.len() as u32;
                let path_len = d.subcommand_path.len() as u16;
                for p in &d.subcommand_path {
                    path_idx.push(pool_b.intern(p));
                }

                let long_off = match d.long {
                    Some(l) => pool_b.intern(&format!("--{l}")),
                    None => 0,
                };
                let desc_off = match d.description {
                    Some(c) if !c.is_empty() => pool_c.intern(&c),
                    _ => 0,
                };

                let mut flags = d.flags.0;
                let (args_idx_val, args_count) = if d.arg_values.is_empty() {
                    (0, 0)
                } else {
                    flags |= EntryFlags::HAS_ARG_VALUES;
                    let i = args_idx.len() as u32;
                    for a in &d.arg_values {
                        args_idx.push(pool_b.intern(a));
                    }
                    (i, d.arg_values.len() as u32)
                };

                packed_entries.push(PackedEntry {
                    short: d.short.unwrap_or(0),
                    flags,
                    path_len,
                    path_idx: path_idx_val,
                    long_off,
                    desc_off,
                    args_idx: args_idx_val,
                    args_count,
                });
            }
            let entries_count = packed_entries.len() as u32 - entries_idx;
            packed_cmds.push(PackedCmd {
                name_off,
                entries_idx,
                entries_count,
            });
        }

        // Section layout
        let cmds_off = HEADER_SIZE;
        let cmds_size = packed_cmds.len() * CMD_RECORD_SIZE;
        let entries_off = cmds_off + cmds_size;
        let entries_size = packed_entries.len() * ENTRY_RECORD_SIZE;
        let path_idx_off = entries_off + entries_size;
        let path_idx_size = path_idx.len() * 4;
        let args_idx_off = path_idx_off + path_idx_size;
        let args_idx_size = args_idx.len() * 4;
        let pool_a_off = args_idx_off + args_idx_size;
        let pool_b_off = pool_a_off + pool_a.bytes.len();
        let pool_c_off = pool_b_off + pool_b.bytes.len();
        let total = pool_c_off + pool_c.bytes.len();

        let mut out = vec![0u8; total];

        // Header
        out[hdr::MAGIC..hdr::MAGIC + 4].copy_from_slice(&MAGIC);
        write_u32(&mut out, hdr::VERSION, VERSION);
        write_u32(&mut out, hdr::CMDS_OFF, cmds_off as u32);
        write_u32(&mut out, hdr::CMDS_COUNT, packed_cmds.len() as u32);
        write_u32(&mut out, hdr::ENTRIES_OFF, entries_off as u32);
        write_u32(&mut out, hdr::ENTRIES_COUNT, packed_entries.len() as u32);
        write_u32(&mut out, hdr::PATH_IDX_OFF, path_idx_off as u32);
        write_u32(&mut out, hdr::PATH_IDX_COUNT, path_idx.len() as u32);
        write_u32(&mut out, hdr::ARGS_IDX_OFF, args_idx_off as u32);
        write_u32(&mut out, hdr::ARGS_IDX_COUNT, args_idx.len() as u32);
        write_u32(&mut out, hdr::POOL_A_OFF, pool_a_off as u32);
        write_u32(&mut out, hdr::POOL_A_LEN, pool_a.bytes.len() as u32);
        write_u32(&mut out, hdr::POOL_B_OFF, pool_b_off as u32);
        write_u32(&mut out, hdr::POOL_B_LEN, pool_b.bytes.len() as u32);
        write_u32(&mut out, hdr::POOL_C_OFF, pool_c_off as u32);
        write_u32(&mut out, hdr::POOL_C_LEN, pool_c.bytes.len() as u32);

        // Command table
        for (i, c) in packed_cmds.iter().enumerate() {
            let base = cmds_off + i * CMD_RECORD_SIZE;
            write_u32(&mut out, base + cmd_rec::NAME_OFF, c.name_off);
            write_u32(&mut out, base + cmd_rec::ENTRIES_IDX, c.entries_idx);
            write_u32(&mut out, base + cmd_rec::ENTRIES_COUNT, c.entries_count);
            write_u32(&mut out, base + cmd_rec::RESERVED, 0);
        }

        // Entry table
        for (i, e) in packed_entries.iter().enumerate() {
            let base = entries_off + i * ENTRY_RECORD_SIZE;
            out[base + entry_rec::SHORT] = e.short;
            out[base + entry_rec::FLAGS] = e.flags;
            write_u16(&mut out, base + entry_rec::PATH_LEN, e.path_len);
            write_u32(&mut out, base + entry_rec::PATH_IDX, e.path_idx);
            write_u32(&mut out, base + entry_rec::LONG_OFF, e.long_off);
            write_u32(&mut out, base + entry_rec::DESC_OFF, e.desc_off);
            write_u32(&mut out, base + entry_rec::ARGS_IDX, e.args_idx);
            write_u32(&mut out, base + entry_rec::ARGS_COUNT, e.args_count);
        }

        // Path index (u32 LE per slot)
        for (i, v) in path_idx.iter().enumerate() {
            write_u32(&mut out, path_idx_off + i * 4, *v);
        }

        // Args index (u32 LE per slot)
        for (i, v) in args_idx.iter().enumerate() {
            write_u32(&mut out, args_idx_off + i * 4, *v);
        }

        // Pools
        out[pool_a_off..pool_a_off + pool_a.bytes.len()].copy_from_slice(&pool_a.bytes);
        out[pool_b_off..pool_b_off + pool_b.bytes.len()].copy_from_slice(&pool_b.bytes);
        out[pool_c_off..pool_c_off + pool_c.bytes.len()].copy_from_slice(&pool_c.bytes);

        out
    }
}

#[derive(Debug)]
struct PackedCmd {
    name_off: u32,
    entries_idx: u32,
    entries_count: u32,
}

#[derive(Debug)]
struct PackedEntry {
    short: u8,
    flags: u8,
    path_len: u16,
    path_idx: u32,
    long_off: u32,
    desc_off: u32,
    args_idx: u32,
    args_count: u32,
}

/// Append-only string pool. Offset 0 is the leading NUL — `intern("")`
/// returns 0, callers treat 0 as "no string".
struct StrPool {
    bytes: Vec<u8>,
    map: HashMap<String, u32>,
}

impl StrPool {
    fn new() -> Self {
        Self {
            bytes: vec![0],
            map: HashMap::new(),
        }
    }

    fn intern(&mut self, s: &str) -> u32 {
        if s.is_empty() {
            return 0;
        }
        if let Some(&off) = self.map.get(s) {
            return off;
        }
        let off = self.bytes.len() as u32;
        self.bytes.extend_from_slice(s.as_bytes());
        self.bytes.push(0);
        self.map.insert(s.to_owned(), off);
        off
    }
}

fn write_u32(out: &mut [u8], offset: usize, v: u32) {
    out[offset..offset + 4].copy_from_slice(&v.to_le_bytes());
}

fn write_u16(out: &mut [u8], offset: usize, v: u16) {
    out[offset..offset + 2].copy_from_slice(&v.to_le_bytes());
}
