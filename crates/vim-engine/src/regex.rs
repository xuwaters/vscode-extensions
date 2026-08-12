//! A small backtracking regex engine speaking Vim's pattern syntax.
//!
//! Vim's dialect is not PCRE: which characters are operators depends on the
//! *magic level* (`\v` very magic, `\m` magic — the default, `\M` nomagic,
//! `\V` very nomagic), so quantifiers are spelled `\+ \? \{n,m}`, groups are
//! `\(…\)`, alternation is `\|`, and `\zs`/`\ze` trim the reported match.
//! Patterns compile to a flat instruction program run by a backtracking VM
//! over one line's chars, which keeps a general regex crate (and its Unicode
//! tables) out of the WASM bundle.
//!
//! Matching never spans a line break: the engine is handed one line at a
//! time, so `^`/`$` anchor to line ends and `\n` simply never matches.

use crate::motion::{CharClass, class};

/// VM steps one whole-line scan may take before giving up. Generous for
/// interactive use, low enough that a catastrophically backtracking pattern
/// (`\(a*\)*b`) fails fast instead of hanging the editor.
pub const DEFAULT_BUDGET: u32 = 400_000;

/// Backtrack points held at once; a bound on memory, not on pattern size.
const MAX_STACK: usize = 50_000;
/// Compiled program size, which bounds `\{n,m}` expansion blowup.
const MAX_PROG: usize = 20_000;
/// Largest `\{n,m}` bound accepted.
const MAX_REPEAT: u32 = 500;
/// How many nullable loops can carry an empty-iteration guard.
const MAX_LOOP_GUARDS: usize = 4;

/// Slots 0/1 hold the match bounds, 2·n/2·n+1 group `n`, and the last two
/// the `\zs` / `\ze` overrides.
const NSLOTS: usize = 22;
const ZS_SLOT: usize = 20;
const ZE_SLOT: usize = 21;
const MAX_GROUPS: usize = 9;
const UNSET: u32 = u32::MAX;

/// Vim's magic levels, ordered by how many characters are operators. The
/// names are Vim's own, hence the shared suffix.
#[allow(clippy::enum_variant_names)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Magic {
    VeryNoMagic,
    NoMagic,
    Magic,
    VeryMagic,
}

/// The magic level at which `c` becomes an operator without a backslash;
/// `None` for characters that are never operators. A backslash flips the
/// verdict, which is exactly how Vim's levels work: `\(` groups in magic
/// mode, `(` groups in very magic and `\(` is then a literal paren.
fn operator_level(c: char) -> Option<Magic> {
    match c {
        // Positional: still special at `\V` (only at branch start / end).
        '^' | '$' => Some(Magic::VeryNoMagic),
        '.' | '*' | '[' => Some(Magic::Magic),
        '+' | '?' | '=' | '{' | '(' | ')' | '|' | '%' | '<' | '>' | '@' | '&' => {
            Some(Magic::VeryMagic)
        }
        _ => None,
    }
}

fn bare_special(c: char, magic: Magic) -> bool {
    operator_level(c).is_some_and(|need| magic >= need)
}

fn is_word(ch: char) -> bool {
    class(ch, false) == CharClass::Word
}

fn chars_eq(a: char, b: char, ignore_case: bool) -> bool {
    a == b || (ignore_case && a.to_lowercase().eq(b.to_lowercase()))
}

// ---- character classes ------------------------------------------------------

/// A named character class (`\w`, `\d`, `[:alpha:]`, …).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Word,
    Digit,
    Space,
    Alpha,
    Alnum,
    Lower,
    Upper,
    Head,
    Hex,
    Octal,
    Punct,
    Print,
}

impl Kind {
    fn holds(self, ch: char) -> bool {
        match self {
            // Keyword chars, matching the word motions' notion (see motion.rs).
            Kind::Word => is_word(ch),
            Kind::Digit => ch.is_ascii_digit(),
            Kind::Space => ch == ' ' || ch == '\t',
            Kind::Alpha => ch.is_ascii_alphabetic(),
            Kind::Alnum => ch.is_ascii_alphanumeric(),
            Kind::Lower => ch.is_lowercase(),
            Kind::Upper => ch.is_uppercase(),
            Kind::Head => ch.is_ascii_alphabetic() || ch == '_',
            Kind::Hex => ch.is_ascii_hexdigit(),
            Kind::Octal => ('0'..='7').contains(&ch),
            Kind::Punct => ch.is_ascii_punctuation(),
            Kind::Print => !ch.is_control(),
        }
    }

    fn from_posix(name: &str) -> Option<Kind> {
        Some(match name {
            "alpha" => Kind::Alpha,
            "alnum" => Kind::Alnum,
            "digit" => Kind::Digit,
            "lower" => Kind::Lower,
            "upper" => Kind::Upper,
            "space" => Kind::Space,
            "punct" => Kind::Punct,
            "xdigit" => Kind::Hex,
            "print" => Kind::Print,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug)]
enum Item {
    Ch(char),
    Range(char, char),
    Kind(Kind),
}

/// A one-character matcher: a `[…]` collection or a `\w`-style class.
#[derive(Clone, Debug)]
struct Class {
    neg: bool,
    items: Vec<Item>,
}

impl Class {
    fn of_kind(kind: Kind, neg: bool) -> Class {
        Class { neg, items: vec![Item::Kind(kind)] }
    }

    fn holds(&self, ch: char, ignore_case: bool) -> bool {
        let hit = |c: char| {
            self.items.iter().any(|item| match *item {
                Item::Ch(x) => x == c,
                Item::Range(a, b) => a <= c && c <= b,
                Item::Kind(k) => k.holds(c),
            })
        };
        let mut found = hit(ch);
        if !found && ignore_case {
            found = ch.to_lowercase().any(hit) || ch.to_uppercase().any(hit);
        }
        found != self.neg
    }
}

// ---- syntax tree ------------------------------------------------------------

#[derive(Clone, Debug)]
enum Node {
    Empty,
    Char(char),
    Any,
    Class(Class),
    Bol,
    Eol,
    WordStart,
    WordEnd,
    /// `\zs` / `\ze`: move the reported start / end of the match.
    MatchStart,
    MatchEnd,
    Seq(Vec<Node>),
    Alt(Vec<Node>),
    /// `Some(n)` captures into group `n`; `None` is `\%(…\)`.
    Group(Option<usize>, Box<Node>),
    Repeat {
        node: Box<Node>,
        min: u32,
        /// `u32::MAX` means unbounded.
        max: u32,
        greedy: bool,
    },
}

/// Can `node` match the empty string? Used to guard star loops that would
/// otherwise spin forever on an empty body (`\(a*\)*`).
fn nullable(node: &Node) -> bool {
    match node {
        Node::Empty
        | Node::Bol
        | Node::Eol
        | Node::WordStart
        | Node::WordEnd
        | Node::MatchStart
        | Node::MatchEnd => true,
        Node::Char(_) | Node::Any | Node::Class(_) => false,
        Node::Seq(v) => v.iter().all(nullable),
        Node::Alt(v) => v.iter().any(nullable),
        Node::Group(_, n) => nullable(n),
        Node::Repeat { node, min, .. } => *min == 0 || nullable(node),
    }
}

// ---- parser -----------------------------------------------------------------

/// One lexed unit. `Nothing` covers the settings escapes (`\c`, `\v`, …),
/// which change how the rest parses but contribute no atom.
enum Tok {
    End,
    Nothing,
    Lit(char),
    Op(char),
    Class(Class),
    MatchStart,
    MatchEnd,
}

/// Parser position; peeking restores the magic level and case flag too, since
/// lexing a settings escape mutates them.
type Mark = (usize, Magic, Option<bool>);

struct Parser {
    src: Vec<char>,
    i: usize,
    magic: Magic,
    groups: usize,
    force_case: Option<bool>,
}

impl Parser {
    fn mark(&self) -> Mark {
        (self.i, self.magic, self.force_case)
    }

    fn reset(&mut self, m: Mark) {
        (self.i, self.magic, self.force_case) = m;
    }

    fn peek(&self) -> Option<char> {
        self.src.get(self.i).copied()
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.i += 1;
            return true;
        }
        false
    }

    fn next_tok(&mut self) -> Result<Tok, String> {
        let Some(c) = self.peek() else {
            return Ok(Tok::End);
        };
        self.i += 1;
        if c != '\\' {
            return Ok(if bare_special(c, self.magic) { Tok::Op(c) } else { Tok::Lit(c) });
        }
        let Some(d) = self.peek() else {
            return Ok(Tok::Lit('\\')); // trailing backslash: literal
        };
        self.i += 1;
        // A backslash flips an operator's magic verdict.
        if operator_level(d).is_some() {
            return Ok(if bare_special(d, self.magic) { Tok::Lit(d) } else { Tok::Op(d) });
        }
        Ok(match d {
            'w' => Tok::Class(Class::of_kind(Kind::Word, false)),
            'W' => Tok::Class(Class::of_kind(Kind::Word, true)),
            'd' => Tok::Class(Class::of_kind(Kind::Digit, false)),
            'D' => Tok::Class(Class::of_kind(Kind::Digit, true)),
            's' => Tok::Class(Class::of_kind(Kind::Space, false)),
            'S' => Tok::Class(Class::of_kind(Kind::Space, true)),
            'a' => Tok::Class(Class::of_kind(Kind::Alpha, false)),
            'A' => Tok::Class(Class::of_kind(Kind::Alpha, true)),
            'l' => Tok::Class(Class::of_kind(Kind::Lower, false)),
            'L' => Tok::Class(Class::of_kind(Kind::Lower, true)),
            'u' => Tok::Class(Class::of_kind(Kind::Upper, false)),
            'U' => Tok::Class(Class::of_kind(Kind::Upper, true)),
            'h' => Tok::Class(Class::of_kind(Kind::Head, false)),
            'H' => Tok::Class(Class::of_kind(Kind::Head, true)),
            'x' => Tok::Class(Class::of_kind(Kind::Hex, false)),
            'X' => Tok::Class(Class::of_kind(Kind::Hex, true)),
            'o' => Tok::Class(Class::of_kind(Kind::Octal, false)),
            'O' => Tok::Class(Class::of_kind(Kind::Octal, true)),
            'z' => match self.peek() {
                Some('s') => {
                    self.i += 1;
                    Tok::MatchStart
                }
                Some('e') => {
                    self.i += 1;
                    Tok::MatchEnd
                }
                _ => return Err(r"\z must be followed by s or e".into()),
            },
            'c' => {
                self.force_case = Some(true);
                Tok::Nothing
            }
            'C' => {
                self.force_case = Some(false);
                Tok::Nothing
            }
            'v' => {
                self.magic = Magic::VeryMagic;
                Tok::Nothing
            }
            'm' => {
                self.magic = Magic::Magic;
                Tok::Nothing
            }
            'M' => {
                self.magic = Magic::NoMagic;
                Tok::Nothing
            }
            'V' => {
                self.magic = Magic::VeryNoMagic;
                Tok::Nothing
            }
            't' => Tok::Lit('\t'),
            'r' => Tok::Lit('\r'),
            'e' => Tok::Lit('\u{1b}'),
            // `\n` is a newline, which a single line never contains.
            'n' => Tok::Lit('\n'),
            other => Tok::Lit(other),
        })
    }

    /// Peek without consuming (or keeping any settings escape's effect).
    fn peek_tok(&mut self) -> Result<Tok, String> {
        let m = self.mark();
        let tok = self.next_tok();
        self.reset(m);
        tok
    }

    fn parse_alt(&mut self) -> Result<Node, String> {
        let mut branches = vec![self.parse_branch()?];
        loop {
            let m = self.mark();
            match self.next_tok()? {
                Tok::Op('|') => branches.push(self.parse_branch()?),
                _ => {
                    self.reset(m);
                    break;
                }
            }
        }
        Ok(if branches.len() == 1 {
            branches.pop().unwrap_or(Node::Empty)
        } else {
            Node::Alt(branches)
        })
    }

    fn parse_branch(&mut self) -> Result<Node, String> {
        let mut items: Vec<Node> = Vec::new();
        loop {
            let m = self.mark();
            let tok = self.next_tok()?;
            let node = match tok {
                Tok::End => break,
                Tok::Op('|') | Tok::Op(')') => {
                    self.reset(m);
                    break;
                }
                Tok::Nothing => continue,
                Tok::Lit(c) => Node::Char(c),
                Tok::Class(cl) => Node::Class(cl),
                Tok::MatchStart => Node::MatchStart,
                Tok::MatchEnd => Node::MatchEnd,
                // `^` anchors only at the start of a branch, `$` only at its
                // end; elsewhere they are literal (vim :help /^).
                Tok::Op('^') => {
                    if items.is_empty() {
                        Node::Bol
                    } else {
                        Node::Char('^')
                    }
                }
                Tok::Op('$') => {
                    if self.at_branch_end()? {
                        Node::Eol
                    } else {
                        Node::Char('$')
                    }
                }
                Tok::Op('.') => Node::Any,
                Tok::Op('<') => Node::WordStart,
                Tok::Op('>') => Node::WordEnd,
                Tok::Op('[') => self.parse_bracket()?,
                Tok::Op('(') => {
                    if self.groups >= MAX_GROUPS {
                        return Err(format!("more than {MAX_GROUPS} groups"));
                    }
                    self.groups += 1;
                    let n = self.groups;
                    let inner = self.parse_alt()?;
                    self.close_group()?;
                    Node::Group(Some(n), Box::new(inner))
                }
                // `\%(…\)`: group without capturing.
                Tok::Op('%') => {
                    if !self.eat('(') {
                        return Err(r"\%( is the only \% form supported".into());
                    }
                    let inner = self.parse_alt()?;
                    self.close_group()?;
                    Node::Group(None, Box::new(inner))
                }
                Tok::Op('@') => return Err(r"look-around (\@) is not supported".into()),
                Tok::Op('&') => return Err(r"\& is not supported".into()),
                // A leading `*` is literal, like Vim's.
                Tok::Op('*') if items.is_empty() => Node::Char('*'),
                Tok::Op(op @ ('*' | '+' | '?' | '=' | '{')) => {
                    let Some(prev) = items.pop() else {
                        return Err(format!("nothing to repeat before '{op}'"));
                    };
                    items.push(self.quantify(prev, op)?);
                    continue;
                }
                Tok::Op(c) => Node::Char(c),
            };
            items.push(node);
        }
        Ok(match items.len() {
            0 => Node::Empty,
            1 => items.pop().unwrap_or(Node::Empty),
            _ => Node::Seq(items),
        })
    }

    fn close_group(&mut self) -> Result<(), String> {
        match self.next_tok()? {
            Tok::Op(')') => Ok(()),
            _ => Err("unmatched ( in pattern".into()),
        }
    }

    /// Is the parser sitting at the end of a branch (so a `$` anchors)?
    fn at_branch_end(&mut self) -> Result<bool, String> {
        Ok(matches!(self.peek_tok()?, Tok::End | Tok::Op('|') | Tok::Op(')')))
    }

    fn quantify(&mut self, node: Node, op: char) -> Result<Node, String> {
        let (min, max, greedy) = match op {
            '*' | '+' => (u32::from(op == '+'), u32::MAX, true),
            '?' | '=' => (0, 1, true),
            _ => self.parse_braces()?,
        };
        Ok(Node::Repeat { node: Box::new(node), min, max, greedy })
    }

    /// The body of `\{…}`: `n`, `n,m`, `n,`, `,m`, empty (= `*`), each
    /// optionally prefixed with `-` for the non-greedy form. Closing brace
    /// may be written `}` or `\}`.
    fn parse_braces(&mut self) -> Result<(u32, u32, bool), String> {
        let greedy = !self.eat('-');
        let min = self.parse_number();
        let comma = self.eat(',');
        let max = if comma { self.parse_number() } else { min };
        self.eat('\\');
        if !self.eat('}') {
            return Err(r"unmatched \{ in pattern".into());
        }
        let min = min.unwrap_or(0);
        let max = max.unwrap_or(u32::MAX);
        if max != u32::MAX && (max < min || max > MAX_REPEAT) {
            return Err(format!(r"invalid \{{{min},{max}}} bound"));
        }
        if min > MAX_REPEAT {
            return Err(format!(r"invalid \{{{min},}} bound"));
        }
        Ok((min, max, greedy))
    }

    fn parse_number(&mut self) -> Option<u32> {
        let start = self.i;
        let mut n: u32 = 0;
        while let Some(d) = self.peek().and_then(|c| c.to_digit(10)) {
            n = n.saturating_mul(10).saturating_add(d);
            self.i += 1;
        }
        (self.i > start).then_some(n)
    }

    /// A `[…]` collection. An unterminated `[` is a literal `[`, like Vim's.
    fn parse_bracket(&mut self) -> Result<Node, String> {
        let open = self.i;
        let mut items = Vec::new();
        let neg = self.eat('^');
        // A `]` right after the (optional) `^` is a literal `]`.
        if self.eat(']') {
            items.push(Item::Ch(']'));
        }
        loop {
            let Some(c) = self.peek() else {
                self.i = open;
                return Ok(Node::Char('['));
            };
            self.i += 1;
            if c == ']' {
                break;
            }
            if c == '[' && self.peek() == Some(':') {
                if let Some(kind) = self.parse_posix_class() {
                    items.push(Item::Kind(kind));
                    continue;
                }
            }
            // Inside a collection only `] \ ^ -` may be backslash-escaped;
            // any other backslash stands for itself (vim :help /[]).
            let c = match (c, self.peek()) {
                ('\\', Some(e @ (']' | '\\' | '^' | '-'))) => {
                    self.i += 1;
                    e
                }
                _ => c,
            };
            match (self.peek(), self.src.get(self.i + 1)) {
                (Some('-'), Some(&hi)) if hi != ']' => {
                    self.i += 2;
                    if hi < c {
                        self.i = open;
                        return Ok(Node::Char('['));
                    }
                    items.push(Item::Range(c, hi));
                }
                _ => items.push(Item::Ch(c)),
            }
        }
        if items.is_empty() {
            self.i = open;
            return Ok(Node::Char('['));
        }
        Ok(Node::Class(Class { neg, items }))
    }

    /// `[:alpha:]` and friends, positioned just after the `[`.
    fn parse_posix_class(&mut self) -> Option<Kind> {
        let start = self.i;
        let rest: String = self.src[self.i + 1..].iter().collect();
        let end = rest.find(":]")?;
        let kind = Kind::from_posix(&rest[..end])?;
        self.i = start + 1 + end + 2;
        Some(kind)
    }
}

// ---- program ----------------------------------------------------------------

#[derive(Clone, Debug)]
enum Inst {
    Char(char),
    Any,
    Class(Class),
    /// Try `.0` first, keep `.1` as a backtrack point.
    Split(usize, usize),
    Jmp(usize),
    Save(usize),
    Bol,
    Eol,
    WordStart,
    WordEnd,
    /// Record / check the input position of a loop iteration, so a loop over
    /// an empty match terminates.
    LoopMark(usize),
    LoopGuard(usize),
    Match,
}

fn emit(node: &Node, prog: &mut Vec<Inst>, loops: &mut usize) -> Result<(), String> {
    if prog.len() > MAX_PROG {
        return Err("pattern too complex".into());
    }
    match node {
        Node::Empty => {}
        Node::Char(c) => prog.push(Inst::Char(*c)),
        Node::Any => prog.push(Inst::Any),
        Node::Class(cl) => prog.push(Inst::Class(cl.clone())),
        Node::Bol => prog.push(Inst::Bol),
        Node::Eol => prog.push(Inst::Eol),
        Node::WordStart => prog.push(Inst::WordStart),
        Node::WordEnd => prog.push(Inst::WordEnd),
        Node::MatchStart => prog.push(Inst::Save(ZS_SLOT)),
        Node::MatchEnd => prog.push(Inst::Save(ZE_SLOT)),
        Node::Seq(v) => {
            for n in v {
                emit(n, prog, loops)?;
            }
        }
        Node::Alt(branches) => {
            let mut jumps = Vec::new();
            for (i, branch) in branches.iter().enumerate() {
                if i + 1 == branches.len() {
                    emit(branch, prog, loops)?;
                    break;
                }
                let split = prog.len();
                prog.push(Inst::Jmp(0)); // patched below
                emit(branch, prog, loops)?;
                jumps.push(prog.len());
                prog.push(Inst::Jmp(0));
                let next = prog.len();
                prog[split] = Inst::Split(split + 1, next);
            }
            let end = prog.len();
            for j in jumps {
                prog[j] = Inst::Jmp(end);
            }
        }
        Node::Group(n, inner) => {
            if let Some(n) = n {
                prog.push(Inst::Save(2 * n));
            }
            emit(inner, prog, loops)?;
            if let Some(n) = n {
                prog.push(Inst::Save(2 * n + 1));
            }
        }
        Node::Repeat { node, min, max, greedy } => {
            for _ in 0..*min {
                emit(node, prog, loops)?;
                if prog.len() > MAX_PROG {
                    return Err("pattern too complex".into());
                }
            }
            if *max == u32::MAX {
                let guard = (nullable(node) && *loops < MAX_LOOP_GUARDS).then(|| {
                    *loops += 1;
                    *loops - 1
                });
                let split = prog.len();
                prog.push(Inst::Jmp(0));
                if let Some(id) = guard {
                    prog.push(Inst::LoopMark(id));
                }
                emit(node, prog, loops)?;
                if let Some(id) = guard {
                    prog.push(Inst::LoopGuard(id));
                }
                prog.push(Inst::Jmp(split));
                let end = prog.len();
                prog[split] = if *greedy {
                    Inst::Split(split + 1, end)
                } else {
                    Inst::Split(end, split + 1)
                };
            } else {
                // Bounded: the optional tail as a run of `X?`.
                for _ in *min..*max {
                    let split = prog.len();
                    prog.push(Inst::Jmp(0));
                    emit(node, prog, loops)?;
                    let end = prog.len();
                    prog[split] = if *greedy {
                        Inst::Split(split + 1, end)
                    } else {
                        Inst::Split(end, split + 1)
                    };
                    if prog.len() > MAX_PROG {
                        return Err("pattern too complex".into());
                    }
                }
            }
        }
    }
    Ok(())
}

// ---- matching ---------------------------------------------------------------

/// Where a match and its groups landed, in char indices into the line.
#[derive(Clone, Debug)]
pub struct Captures {
    slots: [u32; NSLOTS],
}

impl Captures {
    /// Start of the reported match (after any `\zs`).
    pub fn start(&self) -> usize {
        let zs = self.slots[ZS_SLOT];
        (if zs == UNSET { self.slots[0] } else { zs }) as usize
    }

    /// End of the reported match (before any `\ze`).
    pub fn end(&self) -> usize {
        let ze = self.slots[ZE_SLOT];
        (if ze == UNSET { self.slots[1] } else { ze }) as usize
    }

    /// Bounds of group `n`; group 0 is the whole match.
    pub fn group(&self, n: usize) -> Option<(usize, usize)> {
        if n == 0 {
            return Some((self.start(), self.end()));
        }
        let (a, b) = (*self.slots.get(2 * n)?, *self.slots.get(2 * n + 1)?);
        (a != UNSET && b != UNSET && b >= a).then_some((a as usize, b as usize))
    }
}

#[derive(Clone, Copy)]
struct Thread {
    pc: usize,
    sp: u32,
    slots: [u32; NSLOTS],
    marks: [u32; MAX_LOOP_GUARDS],
}

/// A compiled Vim pattern.
#[derive(Clone, Debug)]
pub struct Regex {
    prog: Vec<Inst>,
    ignore_case: bool,
    /// The character a match must start with, when the pattern opens with a
    /// literal. Scanning a line tries every column, so rejecting most of them
    /// with one comparison — no VM state to set up — keeps plain-text search
    /// over a big file as cheap as it was before patterns became regular.
    leading: Option<char>,
}

impl Regex {
    /// Compile `src`. `ignore_case` is the default; `\c` / `\C` in the
    /// pattern override it, as in Vim.
    pub fn new(src: &str, ignore_case: bool) -> Result<Regex, String> {
        let mut p = Parser {
            src: src.chars().collect(),
            i: 0,
            magic: Magic::Magic,
            groups: 0,
            force_case: None,
        };
        let node = p.parse_alt()?;
        if p.i < p.src.len() {
            return Err("unmatched ) in pattern".into());
        }
        let mut prog = vec![Inst::Save(0)];
        let mut loops = 0;
        emit(&node, &mut prog, &mut loops)?;
        prog.push(Inst::Save(1));
        prog.push(Inst::Match);
        // prog[0] is the whole-match `Save`, so prog[1] opens the pattern.
        let leading = match prog.get(1) {
            Some(Inst::Char(c)) => Some(*c),
            _ => None,
        };
        Ok(Regex { prog, ignore_case: p.force_case.unwrap_or(ignore_case), leading })
    }

    pub fn ignore_case(&self) -> bool {
        self.ignore_case
    }

    /// Can a match begin at `at`? A cheap reject, and never a false negative.
    fn can_start_at(&self, chars: &[char], at: usize) -> bool {
        match self.leading {
            Some(c) => chars.get(at).is_some_and(|&x| chars_eq(x, c, self.ignore_case)),
            None => true,
        }
    }

    /// Match starting exactly at `start`. `budget` is decremented per VM step
    /// and shared across a scan, so a whole line can never cost unboundedly.
    pub fn match_at(&self, chars: &[char], start: usize, budget: &mut u32) -> Option<Captures> {
        if !self.can_start_at(chars, start) {
            return None;
        }
        let mut stack: Vec<Thread> = Vec::new();
        let mut th = Thread {
            pc: 0,
            sp: start as u32,
            slots: [UNSET; NSLOTS],
            marks: [UNSET; MAX_LOOP_GUARDS],
        };
        loop {
            if *budget == 0 {
                return None;
            }
            *budget -= 1;
            let sp = th.sp as usize;
            let alive = match &self.prog[th.pc] {
                Inst::Match => return Some(Captures { slots: th.slots }),
                Inst::Char(c) => {
                    let hit = sp < chars.len() && chars_eq(chars[sp], *c, self.ignore_case);
                    if hit {
                        th.pc += 1;
                        th.sp += 1;
                    }
                    hit
                }
                Inst::Any => {
                    let hit = sp < chars.len();
                    if hit {
                        th.pc += 1;
                        th.sp += 1;
                    }
                    hit
                }
                Inst::Class(cl) => {
                    let hit = sp < chars.len() && cl.holds(chars[sp], self.ignore_case);
                    if hit {
                        th.pc += 1;
                        th.sp += 1;
                    }
                    hit
                }
                Inst::Split(a, b) => {
                    if stack.len() >= MAX_STACK {
                        return None;
                    }
                    let mut alt = th;
                    alt.pc = *b;
                    stack.push(alt);
                    th.pc = *a;
                    true
                }
                Inst::Jmp(a) => {
                    th.pc = *a;
                    true
                }
                Inst::Save(slot) => {
                    th.slots[*slot] = th.sp;
                    th.pc += 1;
                    true
                }
                Inst::Bol => {
                    let hit = sp == 0;
                    th.pc += usize::from(hit);
                    hit
                }
                Inst::Eol => {
                    let hit = sp == chars.len();
                    th.pc += usize::from(hit);
                    hit
                }
                Inst::WordStart => {
                    let hit = sp < chars.len()
                        && is_word(chars[sp])
                        && (sp == 0 || !is_word(chars[sp - 1]));
                    th.pc += usize::from(hit);
                    hit
                }
                Inst::WordEnd => {
                    let hit = sp > 0
                        && is_word(chars[sp - 1])
                        && (sp == chars.len() || !is_word(chars[sp]));
                    th.pc += usize::from(hit);
                    hit
                }
                Inst::LoopMark(id) => {
                    th.marks[*id] = th.sp;
                    th.pc += 1;
                    true
                }
                Inst::LoopGuard(id) => {
                    let hit = th.marks[*id] != th.sp;
                    th.pc += usize::from(hit);
                    hit
                }
            };
            if !alive {
                th = stack.pop()?; // nothing left to try: no match here
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    /// First match at or after `from`, as `(start, end)` char indices.
    fn find(pattern: &str, text: &str, from: usize) -> Option<(usize, usize)> {
        let re = Regex::new(pattern, false).expect("compiles");
        let chars: Vec<char> = text.chars().collect();
        let mut budget = DEFAULT_BUDGET;
        (from..=chars.len())
            .find_map(|i| re.match_at(&chars, i, &mut budget))
            .map(|c| (c.start(), c.end()))
    }

    fn matched(pattern: &str, text: &str) -> Option<String> {
        let (s, e) = find(pattern, text, 0)?;
        Some(text.chars().skip(s).take(e - s).collect())
    }

    fn groups(pattern: &str, text: &str) -> Vec<String> {
        let re = Regex::new(pattern, false).expect("compiles");
        let chars: Vec<char> = text.chars().collect();
        let mut budget = DEFAULT_BUDGET;
        let caps = (0..=chars.len())
            .find_map(|i| re.match_at(&chars, i, &mut budget))
            .expect("matches");
        (0..=9)
            .filter_map(|n| caps.group(n))
            .map(|(s, e)| chars[s..e].iter().collect())
            .collect()
    }

    #[test]
    fn literals_and_any() {
        assert_eq!(find("foo", "a foo b", 0), Some((2, 5)));
        assert_eq!(find("f.o", "a fxo b", 0), Some((2, 5)));
        assert_eq!(find(r"a\.b", "axb a.b", 0), Some((4, 7)));
        assert_eq!(find("zz", "abc", 0), None);
    }

    #[test]
    fn anchors_and_word_boundaries() {
        assert_eq!(find("^ab", "abab", 0), Some((0, 2)));
        assert_eq!(find("^ab", "xabab", 0), None);
        assert_eq!(find("ab$", "abab", 0), Some((2, 4)));
        assert_eq!(matched(r"\<foo\>", "foobar foo"), Some("foo".into()));
        // `^` and `$` in the middle of a branch are literal.
        assert_eq!(find("a^b", "xa^b", 0), Some((1, 4)));
        assert_eq!(find(r"a$b", "xa$b", 0), Some((1, 4)));
    }

    #[test]
    fn quantifiers() {
        assert_eq!(matched("ab*", "abbbc"), Some("abbb".into()));
        assert_eq!(matched(r"ab\+", "acabb"), Some("abb".into()));
        assert_eq!(matched(r"ab\?c", "ac"), Some("ac".into()));
        assert_eq!(matched(r"a\{2,3}", "aaaa"), Some("aaa".into()));
        assert_eq!(matched(r"a\{2}", "aaaa"), Some("aa".into()));
        assert_eq!(matched(r"a\{,2}", "aaaa"), Some("aa".into()));
        assert_eq!(matched(r"a\{3,}", "aaaa"), Some("aaaa".into()));
        // Non-greedy.
        assert_eq!(matched(r"a.\{-}b", "axxbxxb"), Some("axxb".into()));
        assert_eq!(matched(r"a.*b", "axxbxxb"), Some("axxbxxb".into()));
        // A leading `*` is a literal star.
        assert_eq!(matched("*x", "a*x"), Some("*x".into()));
    }

    #[test]
    fn groups_and_alternation() {
        assert_eq!(matched(r"foo\|bar", "a bar"), Some("bar".into()));
        assert_eq!(matched(r"\(ab\)\+", "xababy"), Some("abab".into()));
        assert_eq!(groups(r"\(a\+\)\(b\+\)", "xaabbb"), vec!["aabbb", "aa", "bbb"]);
        // Non-capturing groups don't shift the numbering.
        assert_eq!(groups(r"\%(x\)\(y\)", "xy"), vec!["xy", "y"]);
        assert_eq!(Regex::new(r"\(a", false).err(), Some("unmatched ( in pattern".into()));
    }

    #[test]
    fn character_classes() {
        assert_eq!(matched(r"\w\+", "  ab_1 "), Some("ab_1".into()));
        assert_eq!(matched(r"\d\+", "abc 42"), Some("42".into()));
        assert_eq!(matched(r"\s\+", "ab  cd"), Some("  ".into()));
        assert_eq!(matched(r"\u\l\+", "xx Foo"), Some("Foo".into()));
        assert_eq!(matched("[abc]x", "zx bx"), Some("bx".into()));
        assert_eq!(matched("[^abc]x", "bx zx"), Some("zx".into()));
        assert_eq!(matched("[a-c-]x", "zx -x"), Some("-x".into()));
        assert_eq!(matched(r"[[:digit:]]\+", "ab 12"), Some("12".into()));
        assert_eq!(matched(r"[]x]\+", "ab ]x"), Some("]x".into()));
        // An unterminated collection is a literal `[`.
        assert_eq!(matched("[ab", "x[ab"), Some("[ab".into()));
    }

    #[test]
    fn magic_levels() {
        // Very magic: quantifiers and groups without backslashes.
        assert_eq!(matched(r"\v(ab)+", "xabab"), Some("abab".into()));
        assert_eq!(matched(r"\vf.o|bar", "a bar"), Some("bar".into()));
        assert_eq!(matched(r"\v\(a\)", "x(a)"), Some("(a)".into()));
        // Nomagic: `.` and `*` need escaping.
        assert_eq!(matched(r"\Ma.b", "xa.b"), Some("a.b".into()));
        assert_eq!(matched(r"\Ma\.b", "xaxb"), Some("axb".into()));
        // Very nomagic: everything but `^`/`$` is literal.
        assert_eq!(matched(r"\Va.*b", "xa.*b"), Some("a.*b".into()));
        assert_eq!(find(r"\V^ab", "abc", 0), Some((0, 2)));
    }

    #[test]
    fn case_flags_and_ignore_case() {
        assert_eq!(find("foo", "FOO", 0), None);
        assert_eq!(find(r"\cfoo", "xFOO", 0), Some((1, 4)));
        assert_eq!(find(r"foo\c", "xFOO", 0), Some((1, 4)));
        assert_eq!(find(r"\c[a-z]\+", "XY", 0), Some((0, 2)));
        let re = Regex::new("foo", true).expect("compiles");
        assert!(re.ignore_case());
        // `\C` beats the caller's default.
        let re = Regex::new(r"\Cfoo", true).expect("compiles");
        assert!(!re.ignore_case());
    }

    #[test]
    fn zs_and_ze_trim_the_match() {
        assert_eq!(find(r"foo\zsbar", "foobar", 0), Some((3, 6)));
        assert_eq!(find(r"foo\zebar", "foobar", 0), Some((0, 3)));
        assert_eq!(groups(r"\(foo\)\zsbar", "foobar"), vec!["bar", "foo"]);
    }

    #[test]
    fn pathological_patterns_fail_instead_of_hanging() {
        // A nullable star terminates rather than spinning.
        assert_eq!(matched(r"\(a*\)*b", "aaab"), Some("aaab".into()));
        assert_eq!(matched(r"x*", "yyy"), Some("".into()));
        // Exponential backtracking runs out of budget and reports no match.
        let re = Regex::new(r"\(a*\)*b", false).expect("compiles");
        let chars: Vec<char> = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaac".chars().collect();
        let mut budget = DEFAULT_BUDGET;
        assert!(re.match_at(&chars, 0, &mut budget).is_none());
    }

    #[test]
    fn unsupported_syntax_is_reported() {
        assert!(Regex::new(r"\(a\)\@=", false).is_err());
        assert!(Regex::new(r"a\{2,1}", false).is_err());
        assert!(Regex::new(r"a\{2", false).is_err());
        assert!(Regex::new(r"\zx", false).is_err());
    }
}
