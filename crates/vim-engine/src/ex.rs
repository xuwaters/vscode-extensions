//! The `:` command line: line ranges, `:substitute`, and the replacement
//! syntax it expands.
//!
//! Only the search-and-replace corner of ex is implemented — enough for
//! `:%s/\(\w\+\)/[\1]/g` and the addressing that goes with it. Parsing is
//! pure: it turns a typed line plus the cursor's whereabouts into a
//! `Command`, and state.rs applies it to the buffer.

use crate::search::LineMatch;

/// What the cursor and the last visual selection say about line addresses.
pub struct Context {
    /// Cursor line (0-based).
    pub current: usize,
    /// Last line of the buffer (0-based).
    pub last: usize,
    /// Range behind `'<` and `'>`, if a visual selection is in play.
    pub visual: Option<(usize, usize)>,
}

/// A parsed command line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// A bare range (`:12`): move the cursor there.
    Goto(usize),
    Substitute(Substitute),
    /// An empty command line: `:<cr>` does nothing.
    Nothing,
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

/// Parse a command line (without the leading `:`).
pub fn parse(src: &str, ctx: &Context) -> Result<Command, String> {
    let mut p = Cursor::new(src);
    p.skip_blanks();
    let range = parse_range(&mut p, ctx)?;
    p.skip_blanks();
    let name = p.take_while(|c| c.is_ascii_alphabetic());
    if name.is_empty() {
        // A bare address moves the cursor; a bare `:` does nothing.
        return match (p.peek(), range) {
            (None, Some((_, last))) => Ok(Command::Goto(last)),
            (None, None) => Ok(Command::Nothing),
            (Some(c), _) => Err(format!("not an editor command: {c}")),
        };
    }
    if !"substitute".starts_with(&name) {
        return Err(format!("not an editor command: {name}"));
    }
    let (first, last) = range.unwrap_or((ctx.current, ctx.current));
    parse_substitute(&mut p, first, last)
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

/// `%`, `1,5`, `.,+2`, `'<,'>`, … — `None` when no address was typed.
fn parse_range(p: &mut Cursor, ctx: &Context) -> Result<Option<(usize, usize)>, String> {
    if p.eat('%') {
        return Ok(Some((0, ctx.last)));
    }
    let Some(first) = parse_address(p, ctx)? else {
        return Ok(None);
    };
    p.skip_blanks();
    if !p.eat(',') && !p.eat(';') {
        return Ok(Some((first, first)));
    }
    p.skip_blanks();
    let last = parse_address(p, ctx)?.unwrap_or(ctx.current);
    Ok(Some(if first <= last { (first, last) } else { (last, first) }))
}

fn parse_address(p: &mut Cursor, ctx: &Context) -> Result<Option<usize>, String> {
    let mut base: Option<isize> = None;
    if p.eat('.') {
        base = Some(ctx.current as isize);
    } else if p.eat('$') {
        base = Some(ctx.last as isize);
    } else if p.peek().is_some_and(|c| c.is_ascii_digit()) {
        // Addresses are 1-based; `:0` clamps to the first line.
        base = Some(p.number().saturating_sub(1) as isize);
    } else if p.eat('\'') {
        let mark = p.peek().unwrap_or(' ');
        p.bump();
        let (vs, ve) = ctx.visual.ok_or("no visual selection for '< / '>")?;
        base = Some(match mark {
            '<' => vs as isize,
            '>' => ve as isize,
            _ => return Err(format!("marks are not supported: '{mark}")),
        });
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
    Ok(base.map(|line| line.clamp(0, ctx.last as isize) as usize))
}

/// Build the text a match is replaced with. `&` and `\0` stand for the whole
/// match, `\1`…`\9` for capture groups, `\r`/`\n` for a line break, and
/// `\u \l \U \L \E` change the case of what follows (`:help sub-replace`).
pub fn expand(replacement: &str, m: &LineMatch) -> String {
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

    let group = |n: usize| m.groups.get(n).cloned().flatten().unwrap_or_default();
    let mut out = String::new();
    let (mut run, mut once) = (Case::Keep, Case::Keep);
    let mut chars = replacement.chars();
    let mut buf = [0u8; 4];
    while let Some(ch) = chars.next() {
        if ch == '&' {
            push_cased(&mut out, &group(0), run, &mut once);
            continue;
        }
        if ch != '\\' {
            push_cased(&mut out, ch.encode_utf8(&mut buf), run, &mut once);
            continue;
        }
        match chars.next() {
            Some(d @ '0'..='9') => {
                push_cased(&mut out, &group(d as usize - '0' as usize), run, &mut once);
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
        Context { current: 4, last: 9, visual: Some((2, 6)) }
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
        assert!(parse("w", &ctx()).is_err());
        assert!(parse("sort", &ctx()).is_err());
        assert!(parse("s/a/b/c", &ctx()).is_err());
        assert!(parse("s/a/b/z", &ctx()).is_err());
        assert!(parse("'as/a/b/", &ctx()).is_err());
    }

    #[test]
    fn replacements_expand_groups_and_case() {
        let m = LineMatch {
            start: 0,
            end: 7,
            groups: vec![Some("foo=bar".into()), Some("foo".into()), Some("bar".into())],
        };
        assert_eq!(expand("x", &m), "x");
        assert_eq!(expand("&!", &m), "foo=bar!");
        assert_eq!(expand(r"\0!", &m), "foo=bar!");
        assert_eq!(expand(r"\2=\1", &m), "bar=foo");
        assert_eq!(expand(r"\9", &m), "");
        assert_eq!(expand(r"a\&b", &m), "a&b");
        assert_eq!(expand(r"a\rb", &m), "a\nb");
        assert_eq!(expand(r"\u\1", &m), "Foo");
        assert_eq!(expand(r"\U\1\E-\1", &m), "FOO-foo");
        assert_eq!(expand(r"\L\uFOO", &m), "Foo");
    }
}
