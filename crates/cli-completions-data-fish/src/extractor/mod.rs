//! Extractor pipeline: walk the vendored fish snapshot, extract
//! static `complete` directives, encode into a `cli-completions` blob.
//!
//! The pipeline is shared by two consumers:
//!
//! - `build.rs` (via `#[path]`) — runs at compile time, writes
//!   `$OUT_DIR/completions.bin`, which the runtime then `include_bytes!`s.
//! - The library itself — re-exposes the modules so callers can run the
//!   same lexer/parser against ad-hoc `.fish` text (e.g. for tests, or
//!   for a future per-workspace override directory).
//!
//! The "do everything" entry point is [`run`].

pub mod fish_lexer;
pub mod fish_parser;
pub mod predicate;
pub mod snapshot;

use std::path::Path;

use cli_completions::{Builder, DirectiveInput};

use self::fish_parser::{Directive, ParseStats};

/// Result of running the pipeline against a directory of `.fish` files.
pub struct BuildResult {
    /// Encoded blob, ready to be written to `data/completions.bin`.
    pub blob: Vec<u8>,
    /// Aggregate parser statistics (kept/dropped counts, file count).
    pub stats: AggregateStats,
}

/// Aggregate counters across all processed files.
#[derive(Debug, Default, Clone, Copy)]
pub struct AggregateStats {
    /// Number of `.fish` files visited.
    pub files: usize,
    /// Number of `complete` directives kept (passed predicate + has data).
    pub kept: usize,
    /// Number of `complete` directives dropped (parse error, useless,
    /// unsupported predicate, …).
    pub dropped: usize,
}

impl AggregateStats {
    fn add(&mut self, file_stats: &ParseStats) {
        self.files += 1;
        self.kept += file_stats.kept;
        self.dropped += file_stats.dropped;
    }
}

/// Walk `snapshot_dir`, parse every `.fish` file, build a packed blob.
///
/// Iteration is sorted by file name so output is byte-deterministic.
pub fn run(snapshot_dir: &Path) -> std::io::Result<BuildResult> {
    let mut builder = Builder::new();
    let mut stats = AggregateStats::default();

    let files = snapshot::list_fish_files(snapshot_dir)?;
    for file in &files {
        let text = std::fs::read_to_string(&file.path)?;
        let (directives, file_stats) = fish_parser::parse(&text, &file.default_command);
        stats.add(&file_stats);
        for d in &directives {
            push_directive(&mut builder, d);
        }
    }

    Ok(BuildResult {
        blob: builder.build(),
        stats,
    })
}

/// Translate a parsed `Directive` into one or more `DirectiveInput`s on
/// the [`Builder`]. A directive with multiple short / long flags or
/// multiple subcommand-path alternatives expands into several entries.
fn push_directive(builder: &mut Builder, d: &Directive) {
    let paths: Vec<Vec<String>> = if d.subcommand_paths.is_empty() {
        vec![Vec::new()]
    } else {
        d.subcommand_paths.clone()
    };

    let pairs = pair_short_long(&d.shorts, &d.longs);
    let arg_values: Vec<&str> = d.arg_values.iter().map(String::as_str).collect();

    for path in &paths {
        let path_refs: Vec<&str> = path.iter().map(String::as_str).collect();
        for &(short, long) in &pairs {
            builder.add(DirectiveInput {
                command: &d.command,
                short,
                long,
                description: d.description.as_deref(),
                flags: d.flags,
                subcommand_path: &path_refs,
                arg_values: &arg_values,
            });
        }
    }
}

/// Combine N shorts and M longs into (short?, long?) pairs.
///
/// `complete -s v -l verbose` → one paired entry. Mismatched counts emit
/// each form as its own entry. `complete -c foo -a "x y z"` (no flag at
/// all) yields a single `(None, None)` pair so the directive can still
/// surface its arg-value list.
fn pair_short_long<'a>(
    shorts: &'a [u8],
    longs: &'a [String],
) -> Vec<(Option<u8>, Option<&'a str>)> {
    match (shorts.len(), longs.len()) {
        (0, 0) => vec![(None, None)],
        (n, m) if n == m => shorts
            .iter()
            .zip(longs.iter())
            .map(|(s, l)| (Some(*s), Some(l.as_str())))
            .collect(),
        _ => {
            let mut out = Vec::with_capacity(shorts.len() + longs.len());
            for s in shorts {
                out.push((Some(*s), None));
            }
            for l in longs {
                out.push((None, Some(l.as_str())));
            }
            out
        }
    }
}
