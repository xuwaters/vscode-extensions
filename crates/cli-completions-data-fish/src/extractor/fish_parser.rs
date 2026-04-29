//! Parse a `.fish` file into a list of static `complete` directives.
//!
//! The grammar accepted by `complete` is documented at
//! <https://fishshell.com/docs/current/cmds/complete.html>. We implement
//! only the read-only flag forms we care about (RFC 005 §5.1):
//!
//! ```text
//! -c CMD     command this completion belongs to
//! -s S       short option (single char)
//! -l LONG    long option name (no leading --)
//! -d DESC    description
//! -n PRED    predicate (interpreted by predicate.rs)
//! -a LIST    static argument values
//! -x | -f | -r | -F   file-handling flags
//! ```
//!
//! Other forms (`-w`, `-e`, `--keep-order`, `-k`, `--no-files`) are
//! tolerated: short flags we don't model are skipped, while long
//! `--xxx` flags consume one argument blindly. Any `complete` line we
//! cannot parse is counted as dropped but never aborts the whole file.

use cli_completions::EntryFlags;

use super::fish_lexer::{logical_lines, tokenize};
use super::predicate::{interpret, PredicateOutcome};

/// One static directive harvested from a `.fish` file.
#[derive(Debug, Clone)]
pub struct Directive {
    pub command: String,
    pub shorts: Vec<u8>,
    pub longs: Vec<String>,
    pub description: Option<String>,
    pub flags: EntryFlags,
    /// One or more alternative subcommand paths under which the
    /// directive applies. An empty `Vec` means "applies anywhere".
    pub subcommand_paths: Vec<Vec<String>>,
    pub arg_values: Vec<String>,
}

/// Per-file aggregation counters.
#[derive(Debug, Default, Clone, Copy)]
pub struct ParseStats {
    pub kept: usize,
    pub dropped: usize,
}

/// Parse all top-level `complete` directives in `text`.
///
/// `default_command` is the command name derived from the file name
/// (e.g. `curl` for `curl.fish`); used as a fallback when a `complete`
/// line omits `-c`. Pass an empty string if no fallback is desired.
pub fn parse(text: &str, default_command: &str) -> (Vec<Directive>, ParseStats) {
    let mut out = Vec::new();
    let mut stats = ParseStats::default();

    for line in logical_lines(text) {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("complete") {
            continue;
        }

        let tokens = match tokenize(trimmed) {
            Some(t) => t,
            None => {
                stats.dropped += 1;
                continue;
            }
        };
        if tokens.first().map(String::as_str) != Some("complete") {
            continue;
        }

        match parse_complete(&tokens[1..], default_command) {
            ParseResult::Kept(d) => {
                stats.kept += 1;
                out.push(d);
            }
            ParseResult::Dropped => stats.dropped += 1,
        }
    }

    (out, stats)
}

enum ParseResult {
    Kept(Directive),
    Dropped,
}

fn parse_complete(args: &[String], default_command: &str) -> ParseResult {
    let mut command: Option<String> = None;
    let mut shorts: Vec<u8> = Vec::new();
    let mut longs: Vec<String> = Vec::new();
    let mut description: Option<String> = None;
    let mut flags = EntryFlags::default();
    let mut predicate_text: Option<String> = None;
    let mut arg_text: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        let tok = &args[i];

        if tok.starts_with("--") {
            // Long option of `complete` itself, e.g. `--no-files`,
            // `--keep-order`, `--require-parameter`.
            match tok.as_str() {
                "--no-files" | "--exclusive" => flags.insert(EntryFlags::NO_FILES),
                "--require-parameter" => flags.insert(EntryFlags::REQUIRES_ARG),
                "--force-files" => flags.insert(EntryFlags::FORCE_FILES),
                "--description" => {
                    if let Some(v) = args.get(i + 1) {
                        description = Some(v.clone());
                        i += 2;
                        continue;
                    }
                    return ParseResult::Dropped;
                }
                "--command" | "--path" => {
                    if let Some(v) = args.get(i + 1) {
                        command = Some(v.clone());
                        i += 2;
                        continue;
                    }
                    return ParseResult::Dropped;
                }
                "--short-option" => {
                    if let Some(v) = args.get(i + 1) {
                        if let Some(byte) = single_byte(v) {
                            shorts.push(byte);
                        }
                        i += 2;
                        continue;
                    }
                    return ParseResult::Dropped;
                }
                "--long-option" | "--old-option" => {
                    if let Some(v) = args.get(i + 1) {
                        longs.push(v.clone());
                        i += 2;
                        continue;
                    }
                    return ParseResult::Dropped;
                }
                "--condition" => {
                    if let Some(v) = args.get(i + 1) {
                        predicate_text = Some(v.clone());
                        i += 2;
                        continue;
                    }
                    return ParseResult::Dropped;
                }
                "--arguments" => {
                    if let Some(v) = args.get(i + 1) {
                        arg_text.push(v.clone());
                        i += 2;
                        continue;
                    }
                    return ParseResult::Dropped;
                }
                "--keep-order" | "--no-cache" | "--escape" => {
                    i += 1;
                    continue;
                }
                "--erase" | "--do-complete" => {
                    // Not data — abandon this line.
                    return ParseResult::Dropped;
                }
                _ => {
                    // Unknown long flag of complete itself — assume
                    // value-bearing and skip.
                    i += 2;
                    continue;
                }
            }
            i += 1;
            continue;
        }

        if let Some(rest) = tok.strip_prefix('-') {
            if rest.is_empty() {
                // Bare `-`. Skip.
                i += 1;
                continue;
            }
            // Short flag of `complete`. Each char may take an arg.
            // In real fish files we only see one flag per `-X` token,
            // and the flags that take args (-c -s -l -d -n -a -w) take
            // the *next* token as their value.
            //
            // `-x` and `-f` and `-r` and `-F` are standalone; `-k` is
            // `--keep-order` (standalone). `-e` is erase (drop).
            //
            // Fish does allow stacking like `-fc git` (== `-f -c git`),
            // but it's rare. We support it.
            let chars: Vec<char> = rest.chars().collect();
            let mut consumed_next = false;

            for (j, c) in chars.iter().enumerate() {
                let is_last = j == chars.len() - 1;
                match c {
                    'c' if is_last => {
                        if let Some(v) = args.get(i + 1) {
                            command = Some(v.clone());
                            consumed_next = true;
                        } else {
                            return ParseResult::Dropped;
                        }
                    }
                    'p' if is_last => {
                        // `-p PATH` — same role as `-c` for our purposes
                        // (the binary's filesystem path); ignore the
                        // value and continue.
                        if args.get(i + 1).is_some() {
                            consumed_next = true;
                        }
                    }
                    's' if is_last => {
                        if let Some(v) = args.get(i + 1) {
                            if let Some(byte) = single_byte(v) {
                                shorts.push(byte);
                            }
                            consumed_next = true;
                        }
                    }
                    'l' if is_last => {
                        if let Some(v) = args.get(i + 1) {
                            longs.push(v.clone());
                            consumed_next = true;
                        }
                    }
                    'o' if is_last => {
                        // `-o LONG` — old-style long with single dash.
                        // Treat the same as `-l`.
                        if let Some(v) = args.get(i + 1) {
                            longs.push(v.clone());
                            consumed_next = true;
                        }
                    }
                    'd' if is_last => {
                        if let Some(v) = args.get(i + 1) {
                            description = Some(v.clone());
                            consumed_next = true;
                        }
                    }
                    'n' if is_last => {
                        if let Some(v) = args.get(i + 1) {
                            predicate_text = Some(v.clone());
                            consumed_next = true;
                        }
                    }
                    'a' if is_last => {
                        if let Some(v) = args.get(i + 1) {
                            arg_text.push(v.clone());
                            consumed_next = true;
                        }
                    }
                    'w' if is_last => {
                        // `-w WRAPPED_CMD` — wrap completions of another
                        // command. Out of scope (RFC §5.3); just consume
                        // the value.
                        if args.get(i + 1).is_some() {
                            consumed_next = true;
                        }
                    }
                    'k' => { /* keep-order */ }
                    'x' => {
                        flags.insert(EntryFlags::REQUIRES_ARG);
                        flags.insert(EntryFlags::NO_FILES);
                    }
                    'f' => flags.insert(EntryFlags::NO_FILES),
                    'r' => flags.insert(EntryFlags::REQUIRES_ARG),
                    'F' => flags.insert(EntryFlags::FORCE_FILES),
                    'e' => return ParseResult::Dropped, // --erase
                    _ => {
                        // Unknown short — bail on this line.
                        return ParseResult::Dropped;
                    }
                }
            }

            i += 1 + (consumed_next as usize);
            continue;
        }

        // A bareword in `complete`'s arg position is most often the
        // command name when `-c` was omitted (e.g.
        // `complete git -f -n …`). The `complete` builtin accepts this
        // shape: the first non-flag positional is the command.
        if command.is_none() {
            command = Some(tok.clone());
            i += 1;
            continue;
        }

        // Stray positional after the command — fish treats these as
        // additional arguments to `-a`. We do the same.
        arg_text.push(tok.clone());
        i += 1;
    }

    let command = match command.or_else(|| {
        if default_command.is_empty() {
            None
        } else {
            Some(default_command.to_owned())
        }
    }) {
        Some(c) => c,
        None => return ParseResult::Dropped,
    };

    // Resolve predicate.
    let pred = predicate_text.as_deref().map(interpret).unwrap_or(PredicateOutcome::Always);
    let subcommand_paths = match pred {
        PredicateOutcome::Always => Vec::new(),
        PredicateOutcome::Drop => return ParseResult::Dropped,
        PredicateOutcome::TopLevel => {
            flags.insert(EntryFlags::TOP_LEVEL_ONLY);
            Vec::new()
        }
        PredicateOutcome::Paths(p) => p,
    };

    let arg_values = collect_static_arg_values(&arg_text);

    let directive = Directive {
        command,
        shorts,
        longs,
        description: description.filter(|s| !s.is_empty()),
        flags,
        subcommand_paths,
        arg_values,
    };

    if !is_useful(&directive) {
        return ParseResult::Dropped;
    }

    ParseResult::Kept(directive)
}

/// True if the directive carries at least one short, long, or static
/// arg value — otherwise it can't surface a completion.
fn is_useful(d: &Directive) -> bool {
    !d.shorts.is_empty() || !d.longs.is_empty() || !d.arg_values.is_empty()
}

/// Parse `-a` argument lists into static literal words. Drops anything
/// that contains a substitution `(…)` or a variable expansion `$…`.
fn collect_static_arg_values(args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for arg in args {
        for word in arg.split_whitespace() {
            if word.contains('(') || word.starts_with('$') {
                continue;
            }
            // Strip optional `\t<description>` suffix used by some
            // completions (e.g. `add\tAdd files`).
            let core = word.split('\t').next().unwrap_or(word);
            if !core.is_empty() && is_plain_arg(core) {
                out.push(core.to_owned());
            }
        }
    }
    out
}

fn is_plain_arg(s: &str) -> bool {
    s.chars().all(|c| {
        c.is_ascii_alphanumeric()
            || matches!(c, '-' | '_' | '.' | '/' | ':' | '+' | '=' | ',' | '@')
    })
}

fn single_byte(s: &str) -> Option<u8> {
    if s.len() == 1 {
        let b = s.as_bytes()[0];
        if b.is_ascii_graphic() {
            return Some(b);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(line: &str) -> Directive {
        let (mut v, _) = parse(line, "");
        assert_eq!(v.len(), 1, "expected one directive, got {}: {v:?}", v.len());
        v.remove(0)
    }

    #[test]
    fn simple_long_with_desc() {
        let d = one(r#"complete -c curl -l anyauth -d 'use most secure auth'"#);
        assert_eq!(d.command, "curl");
        assert_eq!(d.longs, vec!["anyauth"]);
        assert_eq!(d.shorts, Vec::<u8>::new());
        assert_eq!(d.description.as_deref(), Some("use most secure auth"));
        assert!(d.subcommand_paths.is_empty());
    }

    #[test]
    fn short_only() {
        let d = one("complete -c grep -s v");
        assert_eq!(d.shorts, vec![b'v']);
    }

    #[test]
    fn paired_short_long() {
        let d = one("complete -c grep -s v -l verbose -d 'be loud'");
        assert_eq!(d.shorts, vec![b'v']);
        assert_eq!(d.longs, vec!["verbose"]);
    }

    #[test]
    fn bareword_command_position() {
        // git.fish writes `complete git -f -n …` (no `-c`).
        let d = one("complete git -f -l version -s v -d 'display git version'");
        assert_eq!(d.command, "git");
        assert!(d.flags.contains(EntryFlags::NO_FILES));
        assert_eq!(d.shorts, vec![b'v']);
        assert_eq!(d.longs, vec!["version"]);
    }

    #[test]
    fn fallback_command_from_filename() {
        let (v, _) = parse("complete -l verbose", "frobnicate");
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].command, "frobnicate");
    }

    #[test]
    fn predicate_top_level() {
        let d = one(
            "complete -c git -f -n __fish_git_needs_command -l version -s v -d 'display git version'",
        );
        assert!(d.flags.contains(EntryFlags::TOP_LEVEL_ONLY));
        assert!(d.subcommand_paths.is_empty());
    }

    #[test]
    fn predicate_using_command() {
        let d = one(
            "complete -c git -n '__fish_git_using_command log show diff-tree' -l pretty -d 'pretty format'",
        );
        assert_eq!(
            d.subcommand_paths,
            vec![
                vec!["log".to_owned()],
                vec!["show".to_owned()],
                vec!["diff-tree".to_owned()],
            ]
        );
    }

    #[test]
    fn predicate_unsupported_drops() {
        let (v, stats) = parse(
            "complete -c foo -n 'string match -qr bar' -l x",
            "",
        );
        assert!(v.is_empty());
        assert_eq!(stats.dropped, 1);
    }

    #[test]
    fn arg_values_static_only() {
        let d = one("complete -c tar -l format -x -a 'gnu pax ustar'");
        assert_eq!(d.arg_values, vec!["gnu", "pax", "ustar"]);
        assert!(d.flags.contains(EntryFlags::REQUIRES_ARG));
        assert!(d.flags.contains(EntryFlags::NO_FILES));
    }

    #[test]
    fn arg_values_drop_substitution() {
        let d = one("complete -c git -l c -a '(__fish_git_branches)'");
        assert!(d.arg_values.is_empty());
        // Long flag still kept so the option name surfaces.
        assert_eq!(d.longs, vec!["c"]);
    }

    #[test]
    fn force_files_flag() {
        let d = one("complete -c cat -F -l show-all");
        assert!(d.flags.contains(EntryFlags::FORCE_FILES));
        assert_eq!(d.longs, vec!["show-all"]);

        // -F alone has no flag/arg payload and should drop.
        let (v, stats) = parse("complete -c cat -F", "");
        assert!(v.is_empty());
        assert_eq!(stats.dropped, 1);
    }

    #[test]
    fn multiple_lines_in_one_text() {
        let text = "\
# header comment
complete -c foo -l a
complete -c foo -l b -d 'thing b'
not_a_complete_line
complete -c bar -s x
";
        let (v, stats) = parse(text, "");
        assert_eq!(v.len(), 3);
        assert_eq!(stats.kept, 3);
        assert_eq!(stats.dropped, 0);
    }

    #[test]
    fn line_continuation_combined() {
        let text = "complete -c curl \\\n  -l verbose \\\n  -d 'be loud'\n";
        let d = one(text);
        assert_eq!(d.command, "curl");
        assert_eq!(d.longs, vec!["verbose"]);
        assert_eq!(d.description.as_deref(), Some("be loud"));
    }

    #[test]
    fn useless_directive_dropped() {
        // No long, no short, no static args.
        let (v, stats) = parse("complete -c foo", "");
        assert!(v.is_empty());
        assert_eq!(stats.dropped, 1);
    }
}
