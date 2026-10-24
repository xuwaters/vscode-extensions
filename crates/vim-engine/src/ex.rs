//! The `:` command line: line ranges, the commands that run on them, and
//! the replacement syntax `:substitute` expands.
//!
//! Parsing is pure: it turns a typed line plus the cursor's whereabouts into
//! a `Command`, and state.rs carries it out — on the buffer for the line
//! commands (`:s :d :y :pu :m :t :j :> :< :sort :g`), on the registers and
//! marks (`:reg :marks :delm :k :ju`), or by handing the host a file command
//! (`:w :q :x` and kin), since only the host knows whether a document has
//! unsaved changes.

use serde::Serialize;

use crate::search::LineMatch;

/// What the cursor and the marks say about line addresses.
pub struct Context {
    /// Cursor line (0-based).
    pub current: usize,
    /// Last line of the buffer (0-based).
    pub last: usize,
    /// The line of every mark that is set, `'<` and `'>` among them, behind
    /// addresses like `'a,'b`.
    pub marks: Vec<(char, usize)>,
}

/// A parsed command line. Line numbers are 0-based and inside the buffer
/// unless a field says otherwise.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// A bare range (`:12`): move the cursor there.
    Goto(usize),
    Substitute(Substitute),
    /// `:[range]d[elete] [x] [count]`: the lines go into the register.
    Delete(Lines),
    /// `:[range]y[ank] [x] [count]`.
    Yank(Lines),
    /// `:[line]pu[t][!] [x]`: the register's text as lines below 1-based
    /// line `below`, 0 being above the first, as `:0pu` and `:1pu!` put it.
    Put { below: usize, register: Option<char> },
    /// `:reg[isters] [names]` and `:di[splay] [names]`: list what the
    /// registers hold, only the ones named when there are names.
    Registers { names: Option<String> },
    /// `:marks [names]`.
    Marks { names: Option<String> },
    /// `:delm[arks] {marks}`, or `:delm!` (`all`) for every lowercase mark.
    DeleteMarks { all: bool, names: Vec<char> },
    /// `:[range]ma[rk] {a-zA-Z'}` and `:[range]k{a-zA-Z'}`: set a mark on
    /// the last line of the range, column 0.
    SetMark { mark: char, line: usize },
    /// `:ju[mps]`.
    Jumps,
    /// `:[range]m[ove] {address}`. `below` is the 1-based line the lines go
    /// under, so 0 puts them above the first line, as `:m0` does.
    Move { first: usize, last: usize, below: usize },
    /// `:[range]t {address}` and `:[range]co[py] {address}`, addressed like
    /// `Move`.
    Copy { first: usize, last: usize, below: usize },
    /// `:[range]j[oin][!] [count]`, always over two lines or more. `spaces`
    /// is off for `:j!`, which joins the lines as they are.
    Join { first: usize, last: usize, spaces: bool },
    /// `:[range]>` and `:[range]<`; `times` counts the `>`s typed (`:>>>`
    /// shifts three times).
    Shift { first: usize, last: usize, dedent: bool, times: usize },
    Sort(Sort),
    Global(Global),
    /// `:w :q :x :wq :wa :qa :up` and the rest: carried out by the host.
    File(FileCommand),
    /// `:e[dit][!]` without a file name: read the document back from disk.
    Reload { force: bool },
    /// An empty command line, or one with nothing to do (`:noh`, `:2,2j`).
    Nothing,
}

/// The lines a `:d` or `:y` takes, and the register named for them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lines {
    pub first: usize,
    pub last: usize,
    /// `:d a`; `None` is the unnamed register.
    pub register: Option<char>,
}

/// `:[range]s/pattern/replacement/flags`. `None` fields mean "reuse what was
/// used last time", which is how `:s`, `:s//new/` and `&` are spelled.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Substitute {
    pub first: usize,
    pub last: usize,
    pub pattern: Option<String>,
    pub replacement: Option<String>,
    /// `g`: every match on a line rather than the first.
    pub global: bool,
    /// `i` / `I`: override case sensitivity.
    pub ignore_case: Option<bool>,
    /// `n`: report the match count, change nothing.
    pub count_only: bool,
    /// `e`: stay quiet when the pattern is not found.
    pub quiet: bool,
}

/// `:[range]sor[t][!] [i] [n] [u] [r] [/pattern/]` (`:help :sort`).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Sort {
    pub first: usize,
    pub last: usize,
    /// `!`: the order reversed.
    pub reverse: bool,
    /// `i`: ignore case.
    pub ignore_case: bool,
    /// `n`: on the first decimal number in the line.
    pub numeric: bool,
    /// `u`: keep only the first of a run of equal lines.
    pub unique: bool,
    /// `/pattern/`: sort on what follows the match, or on the match itself
    /// with `r`. `Some("")` is the last search pattern, which `//` spells.
    pub pattern: Option<String>,
    /// `r`: sort on the match rather than what follows it.
    pub on_match: bool,
}

/// `:[range]g[lobal][!]/pattern/cmd` and `:[range]v[global]/pattern/cmd`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Global {
    pub first: usize,
    pub last: usize,
    /// `:v` and `:g!`: run on the lines that do *not* match.
    pub invert: bool,
    /// `None` reuses the last search pattern (`:g//d`).
    pub pattern: Option<String>,
    /// The command for each line. It is parsed once per line, because its
    /// addresses are relative to that line. Empty is Vim's default `:p`.
    pub command: String,
}

/// A file command, as the host needs it. Whether a document has unsaved
/// changes is known only to the host, so this carries what the user asked
/// for and the host decides: refusing `:q` on a modified document, writing
/// for `:x` only when there is something to write.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileCommand {
    pub write: Write,
    /// Close the editor afterwards (`:q`, `:wq`, `:x`).
    pub close: bool,
    /// Every document rather than this one (`:wa`, `:qa`, `:xa`).
    pub all: bool,
    /// `!`: close even with unsaved changes, throwing them away.
    pub force: bool,
}

/// Whether a file command writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Write {
    Never,
    Always,
    /// Only a document with unsaved changes (`:x`, `:up`, `:wa`).
    IfModified,
}

/// The file command a full command name stands for, `bang` being its `!`.
/// Also what `ZZ` (`:x`) and `ZQ` (`:q!`) run. `None` for any other name.
pub fn file_command(name: &str, bang: bool) -> Option<FileCommand> {
    let (write, close, all) = match name {
        "write" => (Write::Always, false, false),
        "update" => (Write::IfModified, false, false),
        "wq" => (Write::Always, true, false),
        "xit" | "exit" => (Write::IfModified, true, false),
        "quit" => (Write::Never, true, false),
        // `:wa` writes the documents with changes, not every one.
        "wall" => (Write::IfModified, false, true),
        "wqall" | "xall" => (Write::IfModified, true, true),
        "qall" | "quitall" => (Write::Never, true, true),
        _ => return None,
    };
    Some(FileCommand { write, close, all, force: bang })
}

/// The commands understood: the full name, how much of it must be typed and
/// whether it takes a `!`, the way `ex_cmds.h` has them. `:sor` is the
/// shortest `:sort` because `:so` is `:source`, `:co` the shortest `:copy`
/// because `:c` is `:change`, and so on.
const COMMANDS: &[(&str, usize, bool)] = &[
    ("substitute", 1, false),
    ("delete", 1, false),
    ("yank", 1, false),
    ("put", 2, true),
    ("registers", 3, false),
    ("display", 2, false),
    ("mark", 2, false),
    ("marks", 5, false),
    ("delmarks", 4, true),
    ("k", 1, false),
    ("jumps", 2, false),
    ("move", 1, false),
    ("copy", 2, false),
    ("t", 1, false),
    ("join", 1, true),
    (">", 1, false),
    ("<", 1, false),
    ("sort", 3, true),
    ("global", 1, true),
    ("vglobal", 1, false),
    ("write", 1, true),
    ("wq", 2, true),
    ("wall", 2, true),
    ("wqall", 3, true),
    ("xit", 1, true),
    ("exit", 3, true),
    ("xall", 2, true),
    ("quit", 1, true),
    ("qall", 2, true),
    ("quitall", 5, true),
    ("update", 2, true),
    ("edit", 1, true),
    ("saveas", 3, true),
    ("nohlsearch", 3, false),
];

/// Parse a command line (without the leading `:`).
pub fn parse(src: &str, ctx: &Context) -> Result<Command, String> {
    let mut p = Cursor::new(src);
    // `::w` is `:w`: Vim skips colons and blanks ahead of the range.
    p.skip_blanks();
    while p.eat(':') {
        p.skip_blanks();
    }
    let (range, zero) = parse_range(&mut p, ctx)?;
    p.skip_blanks();
    let name = match p.peek() {
        Some(c @ ('<' | '>')) => {
            p.bump();
            c.to_string()
        }
        // `:ka` is `:k a` — but `:ke…` is `:keepmarks` and its kin, as in
        // Vim, so mark e has to be set with a space or with `:mark`.
        Some('k') if p.src.get(p.i + 1).is_some_and(|&c| c.is_ascii_alphabetic() && c != 'e') => {
            p.bump();
            "k".to_string()
        }
        _ => p.take_while(|c| c.is_ascii_alphabetic()),
    };
    if name.is_empty() {
        // A bare address moves the cursor; a bare `:` does nothing.
        return match (p.peek(), range) {
            (None, Some(r)) => Ok(Command::Goto(r.last)),
            (None, None) => Ok(Command::Nothing),
            (Some(_), _) => Err(not_a_command(src)),
        };
    }
    let Some(&(full, _, takes_bang)) = COMMANDS
        .iter()
        .find(|(full, min, _)| name.len() >= *min && full.starts_with(name.as_str()))
    else {
        return Err(not_a_command(src));
    };
    // `:s!a!b!` is a substitution with `!` for its separator, not a bang.
    let bang = takes_bang && p.eat('!');
    if !takes_bang && full != "substitute" && p.peek() == Some('!') {
        return Err("E477: No ! allowed".into());
    }
    let current = Range { first: ctx.current, last: ctx.current, addresses: 0 };
    let whole = Range { first: 0, last: ctx.last, addresses: 0 };
    match full {
        "substitute" => {
            let r = range.unwrap_or(current);
            parse_substitute(&mut p, r.first, r.last)
        }
        "delete" | "yank" => {
            p.skip_blanks();
            let register = parse_register(&mut p, true);
            let r = parse_count(&mut p, range.unwrap_or(current), ctx)?;
            end_of_command(&mut p)?;
            let lines = Lines { first: r.first, last: r.last, register };
            Ok(if full == "delete" { Command::Delete(lines) } else { Command::Yank(lines) })
        }
        "move" | "copy" | "t" => {
            let r = range.unwrap_or(current);
            p.skip_blanks();
            // The destination may be 0, above the first line; anything past
            // the last line is out of range, as in Vim.
            let below = match parse_line(&mut p, ctx)? {
                Some(line) if (-1..=ctx.last as isize).contains(&line) => (line + 1) as usize,
                _ => return Err("E16: Invalid range".into()),
            };
            end_of_command(&mut p)?;
            let (first, last) = (r.first, r.last);
            Ok(if full == "move" {
                Command::Move { first, last, below }
            } else {
                Command::Copy { first, last, below }
            })
        }
        "join" => {
            let mut r = parse_count(&mut p, range.unwrap_or(current), ctx)?;
            end_of_command(&mut p)?;
            // One line joins with the one below it, unless both ends were
            // typed (`:2,2j` does nothing) or there is no line below.
            if r.first == r.last {
                if r.addresses >= 2 || r.last == ctx.last {
                    return Ok(Command::Nothing);
                }
                r.last += 1;
            }
            Ok(Command::Join { first: r.first, last: r.last, spaces: !bang })
        }
        ">" | "<" => {
            let shift = if full == ">" { '>' } else { '<' };
            let mut times = 1;
            // `:> >` counts too: Vim skips the blanks between them.
            while {
                p.skip_blanks();
                p.eat(shift)
            } {
                times += 1;
            }
            let r = parse_count(&mut p, range.unwrap_or(current), ctx)?;
            end_of_command(&mut p)?;
            Ok(Command::Shift { first: r.first, last: r.last, dedent: shift == '<', times })
        }
        "sort" => {
            let r = range.unwrap_or(whole);
            parse_sort(&mut p, r.first, r.last, bang)
        }
        "global" | "vglobal" => {
            let r = range.unwrap_or(whole);
            parse_global(&mut p, r.first, r.last, bang || full == "vglobal")
        }
        "put" => {
            p.skip_blanks();
            let register = parse_register(&mut p, false);
            end_of_command(&mut p)?;
            let line = range.map_or(ctx.current, |r| r.last);
            // `:0pu` puts above the first line, the way `:1pu!` does.
            let below = if zero { 0 } else if bang { line } else { line + 1 };
            Ok(Command::Put { below, register })
        }
        "registers" | "display" => {
            no_range(range)?;
            Ok(Command::Registers { names: names_argument(&mut p) })
        }
        "marks" => {
            no_range(range)?;
            Ok(Command::Marks { names: names_argument(&mut p) })
        }
        "delmarks" => {
            no_range(range)?;
            parse_delmarks(&mut p, bang)
        }
        "mark" | "k" => {
            p.skip_blanks();
            let mark = match p.peek() {
                None => return Err("E471: Argument required".into()),
                Some(c) if c.is_ascii_alphabetic() || c == '\'' || c == '`' => c,
                Some(_) => return Err("E191: Argument must be a letter or forward/backward quote".into()),
            };
            p.bump();
            end_of_command(&mut p)?;
            Ok(Command::SetMark { mark, line: range.map_or(ctx.current, |r| r.last) })
        }
        "jumps" => {
            no_range(range)?;
            end_of_command(&mut p)?;
            Ok(Command::Jumps)
        }
        "nohlsearch" => {
            // Matches are highlighted only while a pattern is being typed,
            // so there is never anything left to switch off.
            no_range(range)?;
            end_of_command(&mut p)?;
            Ok(Command::Nothing)
        }
        "saveas" => Err(":saveas is not supported; use File: Save As".into()),
        "edit" => {
            no_range(range)?;
            no_file_name(&mut p)?;
            Ok(Command::Reload { force: bang })
        }
        _ => {
            let file = file_command(full, bang).ok_or_else(|| not_a_command(src))?;
            if file.all {
                no_range(range)?;
            } else if file.write != Write::Never
                && range.is_some_and(|r| (r.first, r.last) != (0, ctx.last))
            {
                // `:5,9w` writes just those lines, which a save cannot do.
                return Err("writing part of a buffer is not supported".into());
            }
            // A range on `:q` is a window number, which means nothing here.
            no_file_name(&mut p)?;
            Ok(Command::File(file))
        }
    }
}

fn not_a_command(src: &str) -> String {
    format!("E492: Not an editor command: {}", src.trim())
}

fn no_range(range: Option<Range>) -> Result<(), String> {
    match range {
        Some(_) => Err("E481: No range allowed".into()),
        None => Ok(()),
    }
}

/// `:w foo`, `:e foo`, `:w !cmd`: writing or opening another file is the
/// host's Save As and Open, which the engine has no way to reach.
fn no_file_name(p: &mut Cursor) -> Result<(), String> {
    p.skip_blanks();
    let rest = p.rest();
    if rest.is_empty() {
        return Ok(());
    }
    Err(format!("file names are not supported: {}", rest.trim_end()))
}

/// Whatever is left must be blanks.
fn end_of_command(p: &mut Cursor) -> Result<(), String> {
    p.skip_blanks();
    let rest = p.rest();
    if rest.is_empty() {
        return Ok(());
    }
    Err(format!("E488: Trailing characters: {rest}"))
}

/// `:d a`, `:y "`, `:pu +`: a register name ahead of the count. A digit
/// starts the count, as in Vim, so a numbered register cannot be named for
/// `:d` and `:y` — it can for `:pu`, which takes no count. The read-only
/// registers (`. : /`) can only be read, so `:d .` is a delete followed by
/// trailing characters, as in Vim.
fn parse_register(p: &mut Cursor, writing: bool) -> Option<char> {
    let ok = |c: char| {
        c.is_ascii_alphabetic()
            || matches!(c, '"' | '-' | '_' | '*' | '+')
            || !writing && (c.is_ascii_digit() || matches!(c, '.' | ':' | '/'))
    };
    let reg = p.peek().filter(|&c| ok(c))?;
    p.bump();
    Some(reg)
}

/// What `:reg` and `:marks` are limited to: every character of the rest of
/// the line, blanks aside; `None` when nothing is left.
fn names_argument(p: &mut Cursor) -> Option<String> {
    let names: String = p.rest().chars().filter(|c| !c.is_whitespace()).collect();
    (!names.is_empty()).then_some(names)
}

/// `:delm a b`, `:delm a-d`, `:delm "^.[]<>`, `:delm!` (mark.c
/// `ex_delmarks`). A range runs between two marks of one kind — lowercase,
/// uppercase or digits — in order.
fn parse_delmarks(p: &mut Cursor, all: bool) -> Result<Command, String> {
    let rest = p.rest();
    if all {
        return match rest.trim() {
            "" => Ok(Command::DeleteMarks { all: true, names: Vec::new() }),
            _ => Err("E474: Invalid argument".into()),
        };
    }
    let chars: Vec<char> = rest.chars().collect();
    if chars.iter().all(|c| c.is_whitespace()) {
        return Err("E471: Argument required".into());
    }
    let kind = |c: char| match c {
        'a'..='z' => 1,
        'A'..='Z' => 2,
        '0'..='9' => 3,
        _ => 0,
    };
    let mut names = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let invalid = || format!("E475: Invalid argument: {}", chars[i..].iter().collect::<String>());
        if kind(c) > 0 && chars.get(i + 1) == Some(&'-') {
            let to = *chars.get(i + 2).ok_or_else(invalid)?;
            if kind(to) != kind(c) || to < c {
                return Err(invalid());
            }
            names.extend(c..=to);
            i += 3;
            continue;
        }
        match c {
            ' ' | '\t' => {}
            c if kind(c) > 0 || "\"^.[]<>".contains(c) => names.push(c),
            _ => return Err(invalid()),
        }
        i += 1;
    }
    Ok(Command::DeleteMarks { all: false, names })
}

/// `:d 3`, `:> 2`: a count after the command starts at the range's last line
/// and covers that many lines (`:help N:`).
fn parse_count(p: &mut Cursor, r: Range, ctx: &Context) -> Result<Range, String> {
    p.skip_blanks();
    if !p.peek().is_some_and(|c| c.is_ascii_digit()) {
        return Ok(r);
    }
    let n = p.number();
    if n == 0 {
        return Err("E939: Positive count required".into());
    }
    let last = r.last.saturating_add(n - 1).min(ctx.last);
    Ok(Range { first: r.last, last, addresses: r.addresses + 1 })
}

fn parse_sort(p: &mut Cursor, first: usize, last: usize, reverse: bool) -> Result<Command, String> {
    let mut sort = Sort { first, last, reverse, ..Sort::default() };
    while let Some(c) = p.peek() {
        p.bump();
        match c {
            ' ' | '\t' => {}
            'i' => sort.ignore_case = true,
            'n' => sort.numeric = true,
            'u' => sort.unique = true,
            'r' => sort.on_match = true,
            // The rest of the line is a comment.
            '"' => break,
            'b' | 'o' | 'x' | 'f' | 'l' => return Err(format!("the {c} flag of :sort is not supported")),
            sep if !sep.is_ascii_alphabetic() && sort.pattern.is_none() => {
                sort.pattern = Some(p.take_until(sep));
                p.eat(sep);
            }
            _ => return Err(format!("E475: Invalid argument: {c}{}", p.rest())),
        }
    }
    Ok(Command::Sort(sort))
}

fn parse_global(p: &mut Cursor, first: usize, last: usize, invert: bool) -> Result<Command, String> {
    p.skip_blanks();
    let pattern = match p.peek() {
        None => return Err("E148: Regular expression missing from :global".into()),
        // `\/` and `\?` stand for the last search pattern, a Vi leftover.
        Some('\\') => {
            p.bump();
            if !p.eat('/') && !p.eat('?') {
                return Err(r"E10: \ should be followed by /, ? or &".into());
            }
            None
        }
        Some(sep) if sep.is_ascii_alphanumeric() => {
            return Err("E146: Regular expressions can't be delimited by letters".into());
        }
        Some(sep) => {
            p.bump();
            let pattern = p.take_until(sep);
            p.eat(sep);
            (!pattern.is_empty()).then_some(pattern)
        }
    };
    Ok(Command::Global(Global { first, last, invert, pattern, command: p.rest() }))
}

fn parse_substitute(p: &mut Cursor, first: usize, last: usize) -> Result<Command, String> {
    let mut sub = Substitute { first, last, ..Substitute::default() };
    p.skip_blanks();
    // `:s` on its own repeats the last substitution.
    let Some(sep) = p.peek() else {
        return Ok(Command::Substitute(sub));
    };
    if sep.is_alphanumeric() || matches!(sep, '\\' | '"' | '|') {
        return Err(format!("invalid separator for :s: {sep}"));
    }
    p.bump();
    let pattern = p.take_until(sep);
    if p.peek() == Some(sep) {
        p.bump();
        let replacement = p.take_until(sep);
        sub.replacement = Some(replacement);
        if p.peek() == Some(sep) {
            p.bump();
        }
    }
    sub.pattern = (!pattern.is_empty()).then_some(pattern);
    parse_flags(p, &mut sub)?;
    Ok(Command::Substitute(sub))
}

fn parse_flags(p: &mut Cursor, sub: &mut Substitute) -> Result<(), String> {
    p.skip_blanks();
    while let Some(c) = p.peek() {
        p.bump();
        match c {
            'g' => sub.global = true,
            'i' => sub.ignore_case = Some(true),
            'I' => sub.ignore_case = Some(false),
            'n' => sub.count_only = true,
            'e' => sub.quiet = true,
            ' ' | '\t' => {}
            'c' => return Err("the c (confirm) flag is not supported".into()),
            _ => return Err(format!("unknown :s flag: {c}")),
        }
    }
    Ok(())
}

/// A typed line range, ordered. `addresses` is how many ends were typed —
/// `%` counts as two, as it does in Vim — which a few commands care about
/// (`:2,2j` does nothing where `:2j` joins two lines).
#[derive(Clone, Copy, Debug)]
struct Range {
    first: usize,
    last: usize,
    addresses: u8,
}

/// `%`, `1,5`, `.,+2`, `'<,'>`, `'a,'b`, … — `None` when no address was
/// typed. The flag says the last address was line 0, above the first line,
/// which only `:pu` makes anything of.
fn parse_range(p: &mut Cursor, ctx: &Context) -> Result<(Option<Range>, bool), String> {
    if p.eat('%') {
        return Ok((Some(Range { first: 0, last: ctx.last, addresses: 2 }), false));
    }
    let first = parse_line(p, ctx)?;
    p.skip_blanks();
    // `,` and `;` both separate; after `;` the second address counts from
    // the first rather than from the cursor (`:5;+2` is 5 to 7).
    let relative = match p.peek() {
        Some(',') => false,
        Some(';') => true,
        _ => {
            let range = first.map(|line| {
                let line = clamp_line(line, ctx);
                Range { first: line, last: line, addresses: 1 }
            });
            return Ok((range, first.is_some_and(|l| l < 0)));
        }
    };
    p.bump();
    p.skip_blanks();
    // A missing first address is the cursor line (`:,+2d`).
    let first = first.map_or(ctx.current, |l| clamp_line(l, ctx));
    let from = Context {
        current: if relative { first } else { ctx.current },
        last: ctx.last,
        marks: ctx.marks.clone(),
    };
    let raw = parse_line(p, &from)?;
    let last = raw.map_or(from.current, |l| clamp_line(l, ctx));
    let (first, last) = if first <= last { (first, last) } else { (last, first) };
    Ok((Some(Range { first, last, addresses: 2 }), raw.is_some_and(|l| l < 0)))
}

/// An address clamped into the buffer: `:0` is the first line and `:99` in
/// a ten-line buffer the last.
fn clamp_line(line: isize, ctx: &Context) -> usize {
    line.clamp(0, ctx.last as isize) as usize
}

/// One address as typed, 0-based and unclamped: `-1` is the `0` that `:m0`
/// and `:t0` go below.
fn parse_line(p: &mut Cursor, ctx: &Context) -> Result<Option<isize>, String> {
    let mut base: Option<isize> = None;
    if p.eat('.') {
        base = Some(ctx.current as isize);
    } else if p.eat('$') {
        base = Some(ctx.last as isize);
    } else if p.peek().is_some_and(|c| c.is_ascii_digit()) {
        // Addresses are 1-based.
        base = Some(p.number().min(isize::MAX as usize) as isize - 1);
    } else if p.eat('\'') {
        let mark = p.peek().unwrap_or(' ');
        p.bump();
        // `''` and `'\`` are one mark, as in a motion.
        let mark = if mark == '`' { '\'' } else { mark };
        let line = ctx.marks.iter().find(|&&(m, _)| m == mark).map(|&(_, l)| l);
        base = Some(line.ok_or("E20: Mark not set")? as isize);
    }
    // `+`/`-` offsets, which may also stand alone (`:+2s/…`).
    while let Some(sign) = p.peek().filter(|&c| c == '+' || c == '-') {
        p.bump();
        let n = if p.peek().is_some_and(|c| c.is_ascii_digit()) {
            p.number() as isize
        } else {
            1
        };
        let from = base.unwrap_or(ctx.current as isize);
        base = Some(if sign == '+' { from + n } else { from - n });
    }
    Ok(base)
}

/// Build the text a match is replaced with. `&` and `\0` stand for the whole
/// match, `\1`…`\9` for capture groups, `\r`/`\n` for a line break, and
/// `\u \l \U \L \E` change the case of what follows (`:help sub-replace`).
/// `line` is the line `m` was found in, which its groups point into.
pub fn expand(replacement: &str, line: &str, m: &LineMatch) -> String {
    /// A case conversion: for the rest of the replacement (`\U`, `\L`) or for
    /// one character (`\u`, `\l`).
    #[derive(Clone, Copy, PartialEq)]
    enum Case {
        Keep,
        Upper,
        Lower,
    }

    fn push_cased(out: &mut String, text: &str, run: Case, once: &mut Case) {
        for ch in text.chars() {
            let mode = match *once {
                Case::Keep => run,
                one => {
                    *once = Case::Keep;
                    one
                }
            };
            match mode {
                Case::Keep => out.push(ch),
                Case::Upper => out.extend(ch.to_uppercase()),
                Case::Lower => out.extend(ch.to_lowercase()),
            }
        }
    }

    let group = |n: usize| m.group(line, n).unwrap_or_default();
    let mut out = String::new();
    let (mut run, mut once) = (Case::Keep, Case::Keep);
    let mut chars = replacement.chars();
    let mut buf = [0u8; 4];
    while let Some(ch) = chars.next() {
        if ch == '&' {
            push_cased(&mut out, group(0), run, &mut once);
            continue;
        }
        if ch != '\\' {
            push_cased(&mut out, ch.encode_utf8(&mut buf), run, &mut once);
            continue;
        }
        match chars.next() {
            Some(d @ '0'..='9') => {
                push_cased(&mut out, group(d as usize - '0' as usize), run, &mut once);
            }
            // `\r` is Vim's line break; `\n` is a NUL there, which is no use
            // in a VSCode document, so it breaks the line too.
            Some('n' | 'r') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('u') => once = Case::Upper,
            Some('l') => once = Case::Lower,
            Some('U') => run = Case::Upper,
            Some('L') => run = Case::Lower,
            Some('e' | 'E') => (run, once) = (Case::Keep, Case::Keep),
            Some(other) => push_cased(&mut out, other.encode_utf8(&mut buf), run, &mut once),
            None => out.push('\\'),
        }
    }
    out
}

/// Expand `~` — the replacement `:s` used last time — inside a replacement.
/// `\~` is a literal tilde. Done before `expand`, so `previous` is already a
/// plain string and nothing recurses.
pub fn expand_tilde(replacement: &str, previous: &str) -> String {
    let mut out = String::new();
    let mut chars = replacement.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '~' => out.push_str(previous),
            '\\' => match chars.next() {
                Some('~') => out.push('~'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            },
            _ => out.push(ch),
        }
    }
    out
}

/// A tiny char cursor over the typed line.
struct Cursor {
    src: Vec<char>,
    i: usize,
}

impl Cursor {
    fn new(src: &str) -> Cursor {
        Cursor { src: src.chars().collect(), i: 0 }
    }

    fn peek(&self) -> Option<char> {
        self.src.get(self.i).copied()
    }

    fn bump(&mut self) {
        self.i += 1;
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.i += 1;
            return true;
        }
        false
    }

    fn skip_blanks(&mut self) {
        while self.peek().is_some_and(|c| c == ' ' || c == '\t') {
            self.i += 1;
        }
    }

    fn number(&mut self) -> usize {
        let mut n: usize = 0;
        while let Some(d) = self.peek().and_then(|c| c.to_digit(10)) {
            n = n.saturating_mul(10).saturating_add(d as usize);
            self.i += 1;
        }
        n
    }

    /// Everything not read yet, which is then read.
    fn rest(&mut self) -> String {
        let start = self.i.min(self.src.len());
        self.i = self.src.len();
        self.src[start..].iter().collect()
    }

    fn take_while(&mut self, pred: impl Fn(char) -> bool) -> String {
        let start = self.i;
        while self.peek().is_some_and(&pred) {
            self.i += 1;
        }
        self.src[start..self.i].iter().collect()
    }

    /// Text up to the next unescaped `sep`, keeping backslashes: `\/` reaches
    /// the pattern compiler as an escaped (and therefore literal) slash.
    fn take_until(&mut self, sep: char) -> String {
        let start = self.i;
        while let Some(c) = self.peek() {
            if c == sep {
                break;
            }
            self.i += if c == '\\' { 2 } else { 1 };
        }
        self.i = self.i.min(self.src.len());
        self.src[start..self.i].iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn ctx() -> Context {
        Context { current: 4, last: 9, marks: vec![('<', 2), ('>', 6), ('b', 7), ('\'', 1)] }
    }

    fn sub(src: &str) -> Substitute {
        match parse(src, &ctx()) {
            Ok(Command::Substitute(s)) => s,
            other => panic!("expected a substitute, got {other:?}"),
        }
    }

    #[test]
    fn ranges_resolve_against_the_cursor() {
        assert_eq!((sub("s/a/b/").first, sub("s/a/b/").last), (4, 4));
        assert_eq!((sub("%s/a/b/").first, sub("%s/a/b/").last), (0, 9));
        assert_eq!((sub("1,3s/a/b/").first, sub("1,3s/a/b/").last), (0, 2));
        assert_eq!((sub(".,$s/a/b/").first, sub(".,$s/a/b/").last), (4, 9));
        assert_eq!((sub(".,+2s/a/b/").first, sub(".,+2s/a/b/").last), (4, 6));
        assert_eq!((sub("-1,.s/a/b/").first, sub("-1,.s/a/b/").last), (3, 4));
        assert_eq!((sub("'<,'>s/a/b/").first, sub("'<,'>s/a/b/").last), (2, 6));
        // Reversed and out-of-range addresses are ordered and clamped.
        assert_eq!((sub("5,2s/a/b/").first, sub("5,2s/a/b/").last), (1, 4));
        assert_eq!((sub("1,99s/a/b/").first, sub("1,99s/a/b/").last), (0, 9));
    }

    #[test]
    fn patterns_replacements_and_flags() {
        let s = sub(r"%s/\(a\)/[\1]/gi");
        assert_eq!(s.pattern.as_deref(), Some(r"\(a\)"));
        assert_eq!(s.replacement.as_deref(), Some(r"[\1]"));
        assert_eq!((s.global, s.ignore_case), (true, Some(true)));
        // Alternate separators, and escaped separators inside the parts.
        let s = sub(r"s#a/b#c#");
        assert_eq!((s.pattern.as_deref(), s.replacement.as_deref()), (Some("a/b"), Some("c")));
        let s = sub(r"s/a\/b/c/");
        assert_eq!(s.pattern.as_deref(), Some(r"a\/b"));
        // Omitted trailing parts.
        let s = sub("s/foo");
        assert_eq!((s.pattern.as_deref(), s.replacement), (Some("foo"), None));
        let s = sub("s/foo/");
        assert_eq!((s.pattern.as_deref(), s.replacement.as_deref()), (Some("foo"), Some("")));
        // An empty pattern reuses the last search.
        assert_eq!(sub("s//x/").pattern, None);
        // Bare `:s` repeats everything.
        assert_eq!((sub("s").pattern, sub("s").replacement), (None, None));
        assert_eq!(sub("substitute/a/b/").pattern.as_deref(), Some("a"));
        assert!(sub("s/a/b/n").count_only);
        assert!(sub("s/a/b/e").quiet);
    }

    #[test]
    fn addresses_and_commands_that_are_not_supported() {
        assert_eq!(parse("7", &ctx()), Ok(Command::Goto(6)));
        assert_eq!(parse("$", &ctx()), Ok(Command::Goto(9)));
        assert_eq!(parse("99", &ctx()), Ok(Command::Goto(9))); // clamped
        assert_eq!(parse("+2", &ctx()), Ok(Command::Goto(6)));
        assert_eq!(parse("", &ctx()), Ok(Command::Nothing));
        assert_eq!(parse("0", &ctx()), Ok(Command::Goto(0)));
        assert!(parse("s/a/b/c", &ctx()).is_err());
        assert!(parse("s/a/b/z", &ctx()).is_err());
        assert!(parse("'as/a/b/", &ctx()).is_err());
        assert_eq!(parse("frob", &ctx()), Err("E492: Not an editor command: frob".into()));
        // Too short to be the command it starts: `:so` is `:source`.
        assert_eq!(parse("so", &ctx()), Err("E492: Not an editor command: so".into()));
        assert_eq!(parse("d!", &ctx()), Err("E477: No ! allowed".into()));
    }

    fn file(write: Write, close: bool, all: bool, force: bool) -> Result<Command, String> {
        Ok(Command::File(FileCommand { write, close, all, force }))
    }

    #[test]
    fn file_commands_take_vims_abbreviations() {
        use Write::*;
        for w in ["w", "wr", "write", "::w", " w "] {
            assert_eq!(parse(w, &ctx()), file(Always, false, false, false), "{w}");
        }
        assert_eq!(parse("w!", &ctx()), file(Always, false, false, true));
        assert_eq!(parse("%w", &ctx()), file(Always, false, false, false));
        assert_eq!(parse("wq", &ctx()), file(Always, true, false, false));
        for x in ["x", "xi", "xit", "exi", "exit"] {
            assert_eq!(parse(x, &ctx()), file(IfModified, true, false, false), "{x}");
        }
        for q in ["q", "qu", "quit"] {
            assert_eq!(parse(q, &ctx()), file(Never, true, false, false), "{q}");
        }
        assert_eq!(parse("q!", &ctx()), file(Never, true, false, true));
        assert_eq!(parse("3q", &ctx()), file(Never, true, false, false));
        for wa in ["wa", "wal", "wall"] {
            assert_eq!(parse(wa, &ctx()), file(IfModified, false, true, false), "{wa}");
        }
        for qa in ["qa", "qall", "quita", "quitall"] {
            assert_eq!(parse(qa, &ctx()), file(Never, true, true, false), "{qa}");
        }
        assert_eq!(parse("qa!", &ctx()), file(Never, true, true, true));
        for xa in ["wqa", "wqall", "xa", "xall"] {
            assert_eq!(parse(xa, &ctx()), file(IfModified, true, true, false), "{xa}");
        }
        for up in ["up", "update"] {
            assert_eq!(parse(up, &ctx()), file(IfModified, false, false, false), "{up}");
        }
        assert_eq!(parse("e", &ctx()), Ok(Command::Reload { force: false }));
        assert_eq!(parse("edit!", &ctx()), Ok(Command::Reload { force: true }));
        assert_eq!(parse("noh", &ctx()), Ok(Command::Nothing));
        assert_eq!(parse("nohlsearch", &ctx()), Ok(Command::Nothing));
        assert!(parse("no", &ctx()).is_err()); // `:no` is `:noremap`
        assert!(parse("ex", &ctx()).is_err());
        assert!(parse("u", &ctx()).is_err()); // `:u` is `:undo`
    }

    #[test]
    fn file_commands_refuse_what_a_save_cannot_do() {
        assert_eq!(parse("w foo.txt", &ctx()), Err("file names are not supported: foo.txt".into()));
        assert_eq!(parse("wq other", &ctx()), Err("file names are not supported: other".into()));
        assert_eq!(parse("e src/x.rs", &ctx()), Err("file names are not supported: src/x.rs".into()));
        assert!(parse("w !sort", &ctx()).is_err());
        assert!(parse("sav x", &ctx()).unwrap_err().contains("not supported"));
        assert!(parse("saveas", &ctx()).unwrap_err().contains("not supported"));
        assert_eq!(parse("1,3w", &ctx()), Err("writing part of a buffer is not supported".into()));
        assert_eq!(parse("2wa", &ctx()), Err("E481: No range allowed".into()));
        assert_eq!(parse("2e", &ctx()), Err("E481: No range allowed".into()));
    }

    fn lines(first: usize, last: usize) -> Lines {
        Lines { first, last, register: None }
    }

    #[test]
    fn line_commands_take_ranges_registers_and_counts() {
        assert_eq!(parse("d", &ctx()), Ok(Command::Delete(lines(4, 4))));
        assert_eq!(parse("%d", &ctx()), Ok(Command::Delete(lines(0, 9))));
        assert_eq!(parse("2,3delete", &ctx()), Ok(Command::Delete(lines(1, 2))));
        // A count starts at the range's last line.
        assert_eq!(parse("d 3", &ctx()), Ok(Command::Delete(lines(4, 6))));
        assert_eq!(parse("2,3d3", &ctx()), Ok(Command::Delete(lines(2, 4))));
        assert_eq!(parse("d 99", &ctx()), Ok(Command::Delete(lines(4, 9))));
        assert_eq!(parse("d 0", &ctx()), Err("E939: Positive count required".into()));
        let named = Lines { first: 4, last: 5, register: Some('a') };
        assert_eq!(parse("y a 2", &ctx()), Ok(Command::Yank(named)));
        assert_eq!(parse("'<,'>y", &ctx()), Ok(Command::Yank(lines(2, 6))));
        assert_eq!(parse("d x y", &ctx()), Err("E488: Trailing characters: y".into()));
        // `;` counts the second address from the first; a missing first one
        // is the cursor line.
        assert_eq!(parse("2;+1d", &ctx()), Ok(Command::Delete(lines(1, 2))));
        assert_eq!(parse(",+1d", &ctx()), Ok(Command::Delete(lines(4, 5))));
    }

    #[test]
    fn move_and_copy_take_a_destination() {
        assert_eq!(parse("m0", &ctx()), Ok(Command::Move { first: 4, last: 4, below: 0 }));
        assert_eq!(parse("m$", &ctx()), Ok(Command::Move { first: 4, last: 4, below: 10 }));
        assert_eq!(parse("1,2mo +1", &ctx()), Ok(Command::Move { first: 0, last: 1, below: 6 }));
        assert_eq!(parse("m-2", &ctx()), Ok(Command::Move { first: 4, last: 4, below: 3 }));
        assert_eq!(parse("t.", &ctx()), Ok(Command::Copy { first: 4, last: 4, below: 5 }));
        assert_eq!(parse("'<,'>co'>", &ctx()), Ok(Command::Copy { first: 2, last: 6, below: 7 }));
        assert_eq!(parse("co 0", &ctx()), Ok(Command::Copy { first: 4, last: 4, below: 0 }));
        assert_eq!(parse("m", &ctx()), Err("E16: Invalid range".into()));
        assert_eq!(parse("t11", &ctx()), Err("E16: Invalid range".into()));
        assert_eq!(parse("m-9", &ctx()), Err("E16: Invalid range".into()));
        assert!(parse("c0", &ctx()).is_err()); // `:c` is `:change`
    }

    #[test]
    fn join_shift_sort_and_global() {
        let join = |first, last, spaces| Ok(Command::Join { first, last, spaces });
        assert_eq!(parse("j", &ctx()), join(4, 5, true));
        assert_eq!(parse("j!", &ctx()), join(4, 5, false));
        assert_eq!(parse("2,5j", &ctx()), join(1, 4, true));
        assert_eq!(parse("j 3", &ctx()), join(4, 6, true));
        assert_eq!(parse("2,2j", &ctx()), Ok(Command::Nothing));
        assert_eq!(parse("$j", &ctx()), Ok(Command::Nothing));

        let shift = |first, last, dedent, times| Ok(Command::Shift { first, last, dedent, times });
        assert_eq!(parse(">", &ctx()), shift(4, 4, false, 1));
        assert_eq!(parse("%<", &ctx()), shift(0, 9, true, 1));
        assert_eq!(parse(">>>", &ctx()), shift(4, 4, false, 3));
        assert_eq!(parse("> > 2", &ctx()), shift(4, 5, false, 2));
        assert!(parse("><", &ctx()).is_err());

        let Ok(Command::Sort(sort)) = parse("sor! i u", &ctx()) else { panic!("sort") };
        assert_eq!((sort.first, sort.last), (0, 9)); // the whole buffer by default
        assert!(sort.reverse && sort.ignore_case && sort.unique && !sort.numeric);
        let Ok(Command::Sort(sort)) = parse("'<,'>sort n /\\d\\+/ r", &ctx()) else { panic!("sort") };
        assert_eq!((sort.first, sort.last), (2, 6));
        assert_eq!(sort.pattern.as_deref(), Some(r"\d\+"));
        assert!(sort.numeric && sort.on_match);
        assert!(parse("sort x", &ctx()).unwrap_err().contains("not supported"));
        assert_eq!(parse("sort q", &ctx()), Err("E475: Invalid argument: q".into()));

        let Ok(Command::Global(g)) = parse("g/foo/s//bar/", &ctx()) else { panic!("global") };
        assert_eq!((g.first, g.last, g.invert), (0, 9, false));
        assert_eq!((g.pattern.as_deref(), g.command.as_str()), (Some("foo"), "s//bar/"));
        let Ok(Command::Global(g)) = parse("g!#a/b#d", &ctx()) else { panic!("global") };
        assert_eq!((g.invert, g.pattern.as_deref(), g.command.as_str()), (true, Some("a/b"), "d"));
        let Ok(Command::Global(g)) = parse(".,$v/x", &ctx()) else { panic!("vglobal") };
        assert_eq!((g.first, g.last, g.invert, g.command.as_str()), (4, 9, true, ""));
        let Ok(Command::Global(g)) = parse(r"g\/d", &ctx()) else { panic!("global") };
        assert_eq!((g.pattern, g.command.as_str()), (None, "d"));
        assert!(parse("g", &ctx()).unwrap_err().starts_with("E148"));
        assert!(parse("gxfoox", &ctx()).is_err());
    }

    #[test]
    fn marks_address_lines() {
        assert_eq!(parse("'b,'<d", &ctx()), Ok(Command::Delete(lines(2, 7))));
        assert_eq!(parse("''", &ctx()), Ok(Command::Goto(1)));
        assert_eq!(parse("'`+1", &ctx()), Ok(Command::Goto(2)));
        assert_eq!(parse("'z", &ctx()), Err("E20: Mark not set".into()));
    }

    #[test]
    fn put_takes_a_line_a_bang_and_any_readable_register() {
        let put = |below, register| Ok(Command::Put { below, register });
        assert_eq!(parse("pu", &ctx()), put(5, None));
        assert_eq!(parse("put!", &ctx()), put(4, None));
        assert_eq!(parse("0pu", &ctx()), put(0, None));
        assert_eq!(parse("0pu!", &ctx()), put(0, None));
        assert_eq!(parse("$pu a", &ctx()), put(10, Some('a')));
        assert_eq!(parse("pu +", &ctx()), put(5, Some('+')));
        assert_eq!(parse("pu .", &ctx()), put(5, Some('.')));
        assert_eq!(parse("pu 3", &ctx()), put(5, Some('3')));
        assert_eq!(parse("pu ab", &ctx()), Err("E488: Trailing characters: b".into()));
        assert!(parse("p", &ctx()).is_err()); // `:p` is `:print`
        // `:d` and `:y` cannot fill a read-only register.
        assert_eq!(parse("d .", &ctx()), Err("E488: Trailing characters: .".into()));
    }

    #[test]
    fn register_and_mark_commands() {
        assert_eq!(parse("reg", &ctx()), Ok(Command::Registers { names: None }));
        assert_eq!(parse("di a b", &ctx()), Ok(Command::Registers { names: Some("ab".into()) }));
        assert_eq!(parse("registers", &ctx()), Ok(Command::Registers { names: None }));
        assert_eq!(parse("2reg", &ctx()), Err("E481: No range allowed".into()));
        assert_eq!(parse("marks", &ctx()), Ok(Command::Marks { names: None }));
        assert_eq!(parse("marks aB", &ctx()), Ok(Command::Marks { names: Some("aB".into()) }));
        assert_eq!(parse("ju", &ctx()), Ok(Command::Jumps));
        let set = |mark, line| Ok(Command::SetMark { mark, line });
        assert_eq!(parse("ka", &ctx()), set('a', 4));
        assert_eq!(parse("3k b", &ctx()), set('b', 2));
        assert_eq!(parse("ma Z", &ctx()), set('Z', 4));
        assert_eq!(parse("'<,'>mark '", &ctx()), set('\'', 6));
        assert!(parse("ke", &ctx()).is_err()); // `:ke` is `:keepmarks`
        assert_eq!(parse("mark", &ctx()), Err("E471: Argument required".into()));
        assert_eq!(parse("k1", &ctx()), Err("E191: Argument must be a letter or forward/backward quote".into()));
        let del = |names: &str| Ok(Command::DeleteMarks { all: false, names: names.chars().collect() });
        assert_eq!(parse("delm a", &ctx()), del("a"));
        assert_eq!(parse("delmarks a-c x \"^.[]<>", &ctx()), del("abcx\"^.[]<>"));
        assert_eq!(parse("delm A-C1-2", &ctx()), del("ABC12"));
        assert_eq!(parse("delm!", &ctx()), Ok(Command::DeleteMarks { all: true, names: Vec::new() }));
        assert_eq!(parse("delm! a", &ctx()), Err("E474: Invalid argument".into()));
        assert_eq!(parse("delm", &ctx()), Err("E471: Argument required".into()));
        assert_eq!(parse("delm c-a", &ctx()), Err("E475: Invalid argument: c-a".into()));
        assert_eq!(parse("delm a-C", &ctx()), Err("E475: Invalid argument: a-C".into()));
        assert_eq!(parse("delm !", &ctx()), Err("E475: Invalid argument: !".into()));
    }

    #[test]
    fn replacements_expand_groups_and_case() {
        let line = "foo=bar";
        let pat = crate::search::Pattern::parse(r"\(\w\+\)=\(\w\+\)").expect("compiles");
        let found = pat.find_all(line, false);
        let m = &found[0];
        let expand = |replacement: &str| expand(replacement, line, m);
        assert_eq!(expand("x"), "x");
        assert_eq!(expand("&!"), "foo=bar!");
        assert_eq!(expand(r"\0!"), "foo=bar!");
        assert_eq!(expand(r"\2=\1"), "bar=foo");
        assert_eq!(expand(r"\9"), "");
        assert_eq!(expand(r"a\&b"), "a&b");
        assert_eq!(expand(r"a\rb"), "a\nb");
        assert_eq!(expand(r"\u\1"), "Foo");
        assert_eq!(expand(r"\U\1\E-\1"), "FOO-foo");
        assert_eq!(expand(r"\L\uFOO"), "Foo");
    }
}
