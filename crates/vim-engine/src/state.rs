//! The modal state machine: modes, counts, operators, registers, and the
//! effects protocol the TypeScript host consumes.
//!
//! The engine mirrors the document (see buffer.rs) and *self-applies* every
//! edit it emits; the host applies the same edits to the real document and
//! suppresses mirroring for those. External changes (insert-mode typing,
//! undo, other extensions) reach the engine through `apply_change`.

use serde::Serialize;

use crate::buffer::{Buffer, Pos, chars_with_cols, utf16_len, utf16_to_byte};
use crate::ex;
use crate::keys::Key;
use crate::motion::{self, FindKind, char_before};
use crate::search::{self, Search};
use crate::textobj::{self, TextObject};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Normal,
    Insert,
    Visual { linewise: bool },
}

impl Mode {
    fn label(self) -> &'static str {
        match self {
            Mode::Normal => "normal",
            Mode::Insert => "insert",
            Mode::Visual { linewise: false } => "visual",
            Mode::Visual { linewise: true } => "visualLine",
        }
    }
}

/// A selection in VSCode coordinates (end-exclusive, UTF-16 columns).
#[derive(Serialize, Clone, Copy, Debug)]
pub struct Selection {
    pub anchor: Pos,
    pub active: Pos,
}

/// A text edit in pre-state coordinates. When several edits are emitted for
/// one key they touch disjoint lines, so the host can hand them to one
/// `editor.edit` transaction verbatim — every range refers to the document
/// as it was before the key. (A `:s` replacement containing `\r` does change
/// the line count, which is why the engine applies them to its own mirror
/// bottom-up.)
#[derive(Serialize, Clone, Debug)]
pub struct Edit {
    pub start: Pos,
    pub end: Pos,
    pub text: String,
}

/// Side effects the engine cannot express as edits; interpreted by the host.
#[derive(Serialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Command {
    Undo,
    Redo,
    #[serde(rename_all = "camelCase")]
    IndentLines {
        start_line: usize,
        end_line: usize,
        dedent: bool,
    },
    Scroll {
        to: &'static str, // "center" | "top" | "bottom"
    },
}

/// Result of one engine call. `selections` is what the editor selection
/// should become (empty = leave it alone); `edits` are applied first.
#[derive(Serialize, Debug)]
pub struct Effects {
    pub mode: &'static str,
    pub selections: Vec<Selection>,
    pub edits: Vec<Edit>,
    pub commands: Vec<Command>,
    /// Keys buffered toward an incomplete command, for the status bar.
    pub pending: String,
    /// A one-line report for the status bar: `:s` counts, ex errors, a search
    /// that found nothing. `None` leaves whatever was shown before.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Op {
    Delete,
    Change,
    Yank,
    Indent,
    Dedent,
}

impl Op {
    fn from_char(ch: char) -> Option<Op> {
        match ch {
            'd' => Some(Op::Delete),
            'c' => Some(Op::Change),
            'y' => Some(Op::Yank),
            '>' => Some(Op::Indent),
            '<' => Some(Op::Dedent),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Awaiting {
    #[default]
    None,
    Find(FindKind),
    Replace,
    G,
    Z,
    Object {
        around: bool,
    },
    /// Typing a `/` or `?` pattern, terminated by `<cr>`.
    Search {
        backward: bool,
    },
    /// Typing an ex command line after `:`, terminated by `<cr>`.
    Ex,
}

#[derive(Default, Debug)]
struct Pending {
    keys: String,
    count1: usize,
    op: Option<Op>,
    count2: usize,
    awaiting: Awaiting,
    /// The `/`, `?` or `:` line typed so far (only while `awaiting` is
    /// `Search` or `Ex`).
    prompt: String,
}

impl Pending {
    fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Keys buffered so far, as the status bar shows them.
    fn display(&self) -> String {
        let mut s = self.keys.clone();
        s.push_str(&self.prompt);
        s
    }

    fn count(&self) -> usize {
        self.count1.max(1) * self.count2.max(1)
    }

    fn has_count(&self) -> bool {
        self.count1 > 0 || self.count2 > 0
    }

    fn add_digit(&mut self, d: usize) {
        let slot = if self.op.is_some() {
            &mut self.count2
        } else {
            &mut self.count1
        };
        *slot = slot.saturating_mul(10).saturating_add(d);
    }
}

#[derive(Clone, Default, Debug)]
struct Register {
    text: String,
    linewise: bool,
}

/// How a motion combines with an operator.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MotionKind {
    Exclusive,
    Inclusive,
    Linewise,
}

/// Sticky column for `j`/`k`; `usize::MAX` means end-of-line (`$`).
const STICKY_EOL: usize = usize::MAX;

pub struct Session {
    buf: Buffer,
    mode: Mode,
    cursor: Pos,
    /// Visual-mode anchor (inclusive char position).
    anchor: Pos,
    desired_col: usize,
    pending: Pending,
    register: Register,
    last_find: Option<(FindKind, char)>,
    last_search: Option<Search>,
    /// The replacement `:s` last used, behind a bare `:s`, `&` and `~`.
    last_replacement: Option<String>,
    /// Line span of the last visual selection, behind `'<` and `'>`.
    last_visual: Option<(usize, usize)>,
    /// Report for the next `Effects`, taken when one is built.
    message: Option<String>,
}

impl Session {
    pub fn new(text: &str) -> Session {
        Session {
            buf: Buffer::from_text(text),
            mode: Mode::Normal,
            cursor: Pos::new(0, 0),
            anchor: Pos::new(0, 0),
            desired_col: 0,
            pending: Pending::default(),
            register: Register::default(),
            last_find: None,
            last_search: None,
            last_replacement: None,
            last_visual: None,
            message: None,
        }
    }

    pub fn mode_label(&self) -> &'static str {
        self.mode.label()
    }

    pub fn cursor(&self) -> Pos {
        self.cursor
    }

    /// Full mirrored text (primarily for tests and resync checks).
    pub fn text(&self) -> String {
        self.buf.text()
    }

    /// Replace the mirror wholesale (document open / desync recovery).
    pub fn reset(&mut self, text: &str, line: usize, col: usize) {
        self.buf = Buffer::from_text(text);
        self.mode = Mode::Normal;
        self.pending = Pending::default();
        self.cursor = self.clamp_normal(Pos::new(line, col));
        self.desired_col = self.cursor.col;
    }

    /// Mirror an external document change (host-applied edits are already
    /// reflected and must not be passed back in).
    pub fn apply_change(&mut self, start: Pos, end: Pos, text: &str) {
        self.buf.apply_change(start, end, text);
        if self.mode != Mode::Insert {
            self.cursor = self.clamp_normal(self.cursor);
        } else {
            self.cursor = self.clamp_insert(self.cursor);
        }
    }

    /// The editor cursor moved for a reason the engine didn't cause (mouse
    /// click, undo, host-side scrolling commands, insert-mode typing).
    pub fn set_position(&mut self, line: usize, col: usize) -> Effects {
        self.pending = Pending::default();
        if matches!(self.mode, Mode::Visual { .. }) {
            self.mode = Mode::Normal;
        }
        let requested = Pos::new(line, col);
        self.cursor = match self.mode {
            Mode::Insert => self.clamp_insert(requested),
            _ => self.clamp_normal(requested),
        };
        self.desired_col = self.cursor.col;
        let selections = if self.cursor != requested {
            self.current_selections()
        } else {
            Vec::new()
        };
        Effects {
            mode: self.mode.label(),
            selections,
            edits: Vec::new(),
            commands: Vec::new(),
            pending: String::new(),
            message: None,
        }
    }

    /// A non-empty selection was made outside the engine (mouse drag):
    /// enter charwise visual around it. Coordinates are VSCode-style
    /// (end-exclusive); the engine keeps inclusive char positions.
    pub fn set_selection(&mut self, anchor: Pos, active: Pos) -> Effects {
        self.pending = Pending::default();
        if anchor <= active {
            self.anchor = self.clamp_normal(anchor);
            self.cursor = self.clamp_normal(char_before(&self.buf, active));
        } else {
            self.anchor = self.clamp_normal(char_before(&self.buf, anchor));
            self.cursor = self.clamp_normal(active);
        }
        self.mode = Mode::Visual { linewise: false };
        self.desired_col = self.cursor.col;
        Effects {
            mode: self.mode.label(),
            selections: Vec::new(), // don't fight the mouse
            edits: Vec::new(),
            commands: Vec::new(),
            pending: String::new(),
            message: None,
        }
    }

    pub fn key(&mut self, key: Key) -> Effects {
        let mut edits = Vec::new();
        let mut commands = Vec::new();
        match self.mode {
            Mode::Insert => self.key_insert(key),
            _ => self.key_normal(key, &mut edits, &mut commands),
        }
        Effects {
            mode: self.mode.label(),
            selections: self.current_selections(),
            edits,
            commands,
            pending: self.pending.display(),
            message: self.message.take(),
        }
    }

    // ---- mode helpers -----------------------------------------------------

    fn clamp_normal(&self, p: Pos) -> Pos {
        let line = p.line.min(self.buf.last_line());
        let len = self.buf.line_len(line);
        let max_col = self.last_char_col(line, len);
        // Snap into a char boundary.
        let col = p.col.min(max_col);
        let byte = crate::buffer::utf16_to_byte(self.buf.line(line), col);
        Pos::new(line, crate::buffer::byte_to_utf16(self.buf.line(line), byte))
    }

    fn clamp_insert(&self, p: Pos) -> Pos {
        let line = p.line.min(self.buf.last_line());
        Pos::new(line, p.col.min(self.buf.line_len(line)))
    }

    /// Column of the last char on a line (0 when empty).
    fn last_char_col(&self, line: usize, len: usize) -> usize {
        if len == 0 {
            return 0;
        }
        chars_with_cols(self.buf.line(line))
            .last()
            .map_or(0, |&(c, _)| c)
    }

    fn enter_insert(&mut self, at: Pos) {
        self.mode = Mode::Insert;
        self.cursor = self.clamp_insert(at);
        self.desired_col = self.cursor.col;
    }

    fn clear_pending(&mut self) {
        self.pending = Pending::default();
    }

    fn current_selections(&self) -> Vec<Selection> {
        match self.mode {
            Mode::Normal | Mode::Insert => vec![Selection {
                anchor: self.cursor,
                active: self.cursor,
            }],
            Mode::Visual { linewise: false } => {
                let (a, c) = (self.anchor, self.cursor);
                if a <= c {
                    vec![Selection {
                        anchor: a,
                        active: motion::right(&self.buf, c, 1),
                    }]
                } else {
                    vec![Selection {
                        anchor: motion::right(&self.buf, a, 1),
                        active: c,
                    }]
                }
            }
            Mode::Visual { linewise: true } => {
                let (top, bottom, forward) = if self.anchor.line <= self.cursor.line {
                    (self.anchor.line, self.cursor.line, true)
                } else {
                    (self.cursor.line, self.anchor.line, false)
                };
                let start = Pos::new(top, 0);
                let end = if bottom + 1 < self.buf.line_count() {
                    Pos::new(bottom + 1, 0)
                } else {
                    Pos::new(bottom, self.buf.line_len(bottom))
                };
                if forward {
                    vec![Selection { anchor: start, active: end }]
                } else {
                    vec![Selection { anchor: end, active: start }]
                }
            }
        }
    }

    // ---- insert mode ------------------------------------------------------

    fn key_insert(&mut self, key: Key) {
        if key == Key::Esc {
            self.mode = Mode::Normal;
            let back = if self.cursor.col > 0 {
                char_before(&self.buf, self.cursor)
            } else {
                self.cursor
            };
            self.cursor = self.clamp_normal(back);
            self.desired_col = self.cursor.col;
        }
        // Everything else in insert mode is handled natively by the editor.
    }

    // ---- normal / visual dispatch ------------------------------------------

    fn key_normal(&mut self, key: Key, edits: &mut Vec<Edit>, commands: &mut Vec<Command>) {
        match self.pending.awaiting {
            Awaiting::Find(kind) => return self.resolve_find(kind, key, edits, commands),
            Awaiting::Replace => return self.resolve_replace(key, edits),
            Awaiting::G => return self.resolve_g(key, edits, commands),
            Awaiting::Z => return self.resolve_z(key, commands),
            Awaiting::Object { around } => return self.resolve_object(around, key, edits, commands),
            Awaiting::Search { backward } => return self.resolve_search(backward, key, edits, commands),
            Awaiting::Ex => return self.resolve_ex(key, edits),
            Awaiting::None => {}
        }

        let visual = matches!(self.mode, Mode::Visual { .. });
        match key {
            Key::Esc => {
                if visual && self.pending.is_empty() {
                    self.mode = Mode::Normal;
                }
                self.clear_pending();
            }
            Key::Char(c @ '0'..='9') if c != '0' || self.pending.has_count() => {
                self.pending.add_digit(c as usize - '0' as usize);
                self.pending.keys.push(c);
            }
            Key::Char(c) if Op::from_char(c).is_some() && !visual => {
                let op = Op::from_char(c).unwrap();
                match self.pending.op {
                    Some(prev) if prev == op => self.doubled_operator(op, edits, commands),
                    Some(_) => self.clear_pending(),
                    None => {
                        self.pending.op = Some(op);
                        self.pending.keys.push(c);
                    }
                }
            }
            Key::Char(c) if Op::from_char(c).is_some() && visual => {
                self.visual_operator(Op::from_char(c).unwrap(), edits, commands)
            }
            Key::Char('g') => {
                self.pending.awaiting = Awaiting::G;
                self.pending.keys.push('g');
            }
            Key::Char('z') if self.pending.op.is_none() && !visual => {
                self.pending.awaiting = Awaiting::Z;
                self.pending.keys.push('z');
            }
            Key::Char(c @ ('i' | 'a')) if self.pending.op.is_some() || visual => {
                self.pending.awaiting = Awaiting::Object { around: c == 'a' };
                self.pending.keys.push(c);
            }
            Key::Char(c @ ('f' | 'F' | 't' | 'T')) => {
                self.pending.awaiting = Awaiting::Find(match c {
                    'f' => FindKind::To,
                    'F' => FindKind::ToBack,
                    't' => FindKind::Till,
                    _ => FindKind::TillBack,
                });
                self.pending.keys.push(c);
            }
            Key::Char(c @ (';' | ',')) => {
                if let Some((kind, target)) = self.last_find {
                    let kind = if c == ',' { kind.reversed() } else { kind };
                    self.finish_find(kind, target, edits, commands);
                } else {
                    self.clear_pending();
                }
            }
            Key::Char('r') => {
                self.pending.awaiting = Awaiting::Replace;
                self.pending.keys.push('r');
            }
            Key::Char(c @ ('/' | '?')) => {
                self.pending.awaiting = Awaiting::Search { backward: c == '?' };
                self.pending.keys.push(c);
            }
            Key::Char(':') => {
                self.pending.awaiting = Awaiting::Ex;
                self.pending.keys.push(':');
                // `:` in visual mode prefills the selection's range, as Vim's does.
                if visual {
                    self.pending.prompt.push_str("'<,'>");
                }
            }
            _ => self.simple_key(key, edits, commands),
        }
    }

    /// Keys that resolve immediately: motions and standalone actions.
    fn simple_key(&mut self, key: Key, edits: &mut Vec<Edit>, commands: &mut Vec<Command>) {
        let count = self.pending.count();
        let visual = matches!(self.mode, Mode::Visual { .. });
        let cur = self.cursor;
        match key {
            // -- horizontal motions
            Key::Char('h') | Key::Backspace => {
                self.do_motion(motion::left(cur, count), MotionKind::Exclusive, edits, commands)
            }
            Key::Char('l') | Key::Char(' ') => self.do_motion(
                motion::right(&self.buf, cur, count),
                MotionKind::Exclusive,
                edits,
                commands,
            ),
            Key::Char('0') => {
                self.do_motion(Pos::new(cur.line, 0), MotionKind::Exclusive, edits, commands)
            }
            Key::Char('^') => self.do_motion(
                Pos::new(cur.line, self.buf.first_non_blank(cur.line)),
                MotionKind::Exclusive,
                edits,
                commands,
            ),
            Key::Char('$') => {
                let line = (cur.line + count - 1).min(self.buf.last_line());
                let target = Pos::new(line, self.buf.line_len(line));
                self.do_motion(target, MotionKind::Exclusive, edits, commands);
                self.desired_col = STICKY_EOL;
            }
            // -- word motions
            Key::Char(c @ ('w' | 'W')) => self.motion_word_forward(c == 'W', count, edits, commands),
            Key::Char(c @ ('b' | 'B')) => self.do_motion(
                motion::word_back(&self.buf, cur, c == 'B', count),
                MotionKind::Exclusive,
                edits,
                commands,
            ),
            Key::Char(c @ ('e' | 'E')) => self.do_motion(
                motion::word_end(&self.buf, cur, c == 'E', count),
                MotionKind::Inclusive,
                edits,
                commands,
            ),
            // -- vertical motions
            Key::Char('j') => self.vertical(count as isize, false, edits, commands),
            Key::Char('k') => self.vertical(-(count as isize), false, edits, commands),
            Key::Enter | Key::Char('+') => self.vertical(count as isize, true, edits, commands),
            Key::Char('-') => self.vertical(-(count as isize), true, edits, commands),
            Key::Char('G') => {
                let line = if self.pending.has_count() {
                    (count - 1).min(self.buf.last_line())
                } else {
                    self.buf.last_line()
                };
                let target = Pos::new(line, self.buf.first_non_blank(line));
                self.do_motion(target, MotionKind::Linewise, edits, commands);
            }
            Key::Char(c @ ('{' | '}')) => self.do_motion(
                motion::paragraph(&self.buf, cur, c == '}', count),
                MotionKind::Exclusive,
                edits,
                commands,
            ),
            Key::Char('%') => match motion::matching_pair(&self.buf, cur) {
                Some(target) => self.do_motion(target, MotionKind::Inclusive, edits, commands),
                None => self.clear_pending(),
            },
            // -- search motions
            Key::Char(c @ ('n' | 'N')) => match self.last_search.clone() {
                Some(last) => {
                    let backward = last.backward != (c == 'N');
                    self.run_search(&last.pattern, backward, edits, commands);
                }
                None => self.clear_pending(),
            },
            Key::Char(c @ ('*' | '#')) => self.search_word(c == '#', edits, commands),

            // A pending operator combines only with the motions above; any
            // other key aborts it, like Vim.
            _ if self.pending.op.is_some() => self.clear_pending(),

            // -- operatorless actions (normal mode)
            Key::Char('x') if !visual => self.delete_chars(count, edits, false),
            Key::Char('X') if !visual => self.delete_back(count, edits),
            Key::Char('s') if !visual => self.delete_chars(count, edits, true),
            Key::Char('D') if !visual => self.delete_to_eol(edits, false),
            Key::Char('C') if !visual => self.delete_to_eol(edits, true),
            Key::Char('Y') if !visual => self.linewise_yank(count),
            Key::Char('S') if !visual => self.linewise_change(cur.line, count, edits),
            Key::Char('~') if !visual => self.toggle_case(count, edits),
            // `&`: the last `:s` again, on this line, without its flags.
            Key::Char('&') if !visual => {
                let line = cur.line;
                self.substitute(
                    &ex::Substitute { first: line, last: line, ..ex::Substitute::default() },
                    edits,
                );
            }
            Key::Char('J') => self.join_lines(count, edits),
            Key::Char('p') => self.paste(false, count, edits),
            Key::Char('P') => self.paste(true, count, edits),
            Key::Char('u') if !visual => {
                for _ in 0..count {
                    commands.push(Command::Undo);
                }
                self.clear_pending();
            }
            Key::Ctrl('r') => {
                for _ in 0..count {
                    commands.push(Command::Redo);
                }
                self.clear_pending();
            }

            // -- insert entries
            Key::Char('i') if !visual => self.enter_insert_cleared(cur),
            Key::Char('a') if !visual => {
                let at = motion::right(&self.buf, cur, 1);
                self.enter_insert_cleared(at)
            }
            Key::Char('I') if !visual => {
                self.enter_insert_cleared(Pos::new(cur.line, self.buf.first_non_blank(cur.line)))
            }
            Key::Char('A') if !visual => {
                self.enter_insert_cleared(Pos::new(cur.line, self.buf.line_len(cur.line)))
            }
            Key::Char('o') if !visual => {
                let eol = Pos::new(cur.line, self.buf.line_len(cur.line));
                self.emit_edit(eol, eol, "\n", edits);
                self.enter_insert_cleared(Pos::new(cur.line + 1, 0));
            }
            Key::Char('O') if !visual => {
                let bol = Pos::new(cur.line, 0);
                self.emit_edit(bol, bol, "\n", edits);
                self.enter_insert_cleared(Pos::new(cur.line, 0));
            }

            // -- visual mode entry / manipulation
            Key::Char('v') => self.toggle_visual(false),
            Key::Char('V') => self.toggle_visual(true),
            Key::Char('o') if visual => {
                std::mem::swap(&mut self.anchor, &mut self.cursor);
                self.desired_col = self.cursor.col;
                self.clear_pending();
            }

            // -- visual operator synonyms
            Key::Char('x') if visual => self.visual_operator(Op::Delete, edits, commands),
            Key::Char('s') if visual => self.visual_operator(Op::Change, edits, commands),
            Key::Char('D' | 'X') if visual => {
                self.force_linewise_visual();
                self.visual_operator(Op::Delete, edits, commands)
            }
            Key::Char('C' | 'S') if visual => {
                self.force_linewise_visual();
                self.visual_operator(Op::Change, edits, commands)
            }
            Key::Char('Y') if visual => {
                self.force_linewise_visual();
                self.visual_operator(Op::Yank, edits, commands)
            }
            Key::Char('~') if visual => self.visual_toggle_case(edits),

            _ => self.clear_pending(),
        }
    }

    fn enter_insert_cleared(&mut self, at: Pos) {
        self.clear_pending();
        self.enter_insert(at);
    }

    fn toggle_visual(&mut self, linewise: bool) {
        self.clear_pending();
        match self.mode {
            Mode::Visual { linewise: cur } if cur == linewise => self.mode = Mode::Normal,
            Mode::Visual { .. } => self.mode = Mode::Visual { linewise },
            _ => {
                self.anchor = self.cursor;
                self.mode = Mode::Visual { linewise };
            }
        }
    }

    fn force_linewise_visual(&mut self) {
        if let Mode::Visual { linewise } = &mut self.mode {
            *linewise = true;
        }
    }

    // ---- motion plumbing ---------------------------------------------------

    fn motion_word_forward(
        &mut self,
        big: bool,
        count: usize,
        edits: &mut Vec<Edit>,
        commands: &mut Vec<Command>,
    ) {
        let cur = self.cursor;
        // `cw`/`cW` on a non-blank acts like `ce` (vim :help cw).
        if self.pending.op == Some(Op::Change)
            && self.buf.char_at(cur).is_some_and(|ch| !ch.is_whitespace())
        {
            let target = motion::word_end(&self.buf, cur, big, count);
            return self.do_motion(target, MotionKind::Inclusive, edits, commands);
        }
        let mut target = motion::word_forward(&self.buf, cur, big, count);
        // Operator + w stops at end of line when the last word moved over
        // ends there (vim :help word), instead of eating the newline.
        if self.pending.op.is_some() && target.line > cur.line {
            let eol = Pos::new(cur.line, self.buf.line_len(cur.line));
            let crossed = self.buf.slice(eol, target);
            let line_had_word = self
                .buf
                .slice(cur, eol)
                .chars()
                .any(|ch| !ch.is_whitespace());
            if line_had_word && crossed.chars().all(char::is_whitespace) {
                target = eol;
            }
        }
        self.do_motion(target, MotionKind::Exclusive, edits, commands);
    }

    fn vertical(
        &mut self,
        dy: isize,
        first_non_blank: bool,
        edits: &mut Vec<Edit>,
        commands: &mut Vec<Command>,
    ) {
        let desired = self.desired_col;
        let had_op = self.pending.op.is_some();
        let line = self
            .cursor
            .line
            .saturating_add_signed(dy)
            .min(self.buf.last_line());
        let col = if first_non_blank {
            self.buf.first_non_blank(line)
        } else {
            let len = self.buf.line_len(line);
            desired.min(self.last_char_col(line, len))
        };
        self.do_motion(Pos::new(line, col), MotionKind::Linewise, edits, commands);
        if !first_non_blank && !had_op {
            self.desired_col = desired;
        }
    }

    /// Route a resolved motion: either move the cursor or feed the pending
    /// operator. `target` may sit one past the last char (exclusive end).
    fn do_motion(
        &mut self,
        target: Pos,
        kind: MotionKind,
        edits: &mut Vec<Edit>,
        commands: &mut Vec<Command>,
    ) {
        if let Some(op) = self.pending.op {
            let (s, e) = ordered(self.cursor, target);
            let (s, e) = match kind {
                MotionKind::Exclusive => (s, e),
                MotionKind::Inclusive => (s, motion::right(&self.buf, e, 1)),
                MotionKind::Linewise => {
                    self.apply_operator_linewise(op, s.line, e.line, edits, commands);
                    self.clear_pending();
                    self.desired_col = self.cursor.col;
                    return;
                }
            };
            self.apply_operator_charwise(op, s, e, edits, commands);
            self.clear_pending();
            self.desired_col = self.cursor.col;
        } else {
            self.cursor = match self.mode {
                Mode::Insert => self.clamp_insert(target),
                _ => self.clamp_normal(target),
            };
            self.desired_col = self.cursor.col;
            self.clear_pending();
        }
    }

    // ---- awaiting resolutions ----------------------------------------------

    fn resolve_find(
        &mut self,
        kind: FindKind,
        key: Key,
        edits: &mut Vec<Edit>,
        commands: &mut Vec<Command>,
    ) {
        match key {
            Key::Char(c) => {
                self.last_find = Some((kind, c));
                self.finish_find(kind, c, edits, commands);
            }
            _ => self.clear_pending(),
        }
    }

    fn finish_find(
        &mut self,
        kind: FindKind,
        target: char,
        edits: &mut Vec<Edit>,
        commands: &mut Vec<Command>,
    ) {
        let count = self.pending.count();
        match motion::find_char(&self.buf, self.cursor, kind, target, count) {
            Some(pos) => {
                let mk = if kind.forward() {
                    MotionKind::Inclusive
                } else {
                    MotionKind::Exclusive
                };
                self.pending.awaiting = Awaiting::None;
                self.do_motion(pos, mk, edits, commands);
            }
            None => self.clear_pending(),
        }
    }

    /// Collect the `/` or `?` pattern until `<cr>` runs it. `<esc>`, and
    /// `<bs>` past the start of the pattern, abort the search like Vim.
    fn resolve_search(
        &mut self,
        backward: bool,
        key: Key,
        edits: &mut Vec<Edit>,
        commands: &mut Vec<Command>,
    ) {
        match key {
            Key::Char(c) => self.pending.prompt.push(c),
            Key::Backspace => {
                if self.pending.prompt.pop().is_none() {
                    self.clear_pending();
                }
            }
            Key::Enter => {
                // An empty pattern reuses the last one.
                let pattern = match (self.pending.prompt.as_str(), &self.last_search) {
                    ("", None) => return self.clear_pending(),
                    ("", Some(last)) => last.pattern.clone(),
                    (typed, _) => typed.to_string(),
                };
                self.pending.awaiting = Awaiting::None;
                self.pending.prompt.clear();
                self.last_search = Some(Search {
                    pattern: pattern.clone(),
                    backward,
                });
                self.run_search(&pattern, backward, edits, commands);
            }
            Key::Esc => self.clear_pending(),
            Key::Ctrl(_) => {}
        }
    }

    /// `*` / `#`: search for the keyword under the cursor, whole-word. Vim
    /// first parks the cursor on the keyword's first char, which is what
    /// keeps the search from matching the keyword the cursor sits in.
    fn search_word(&mut self, backward: bool, edits: &mut Vec<Edit>, commands: &mut Vec<Command>) {
        let Some((start, word)) = search::word_under_cursor(&self.buf, self.cursor) else {
            return self.clear_pending();
        };
        self.cursor = start;
        self.desired_col = start.col;
        let pattern = format!("\\<{}\\>", search::escape_literal(&word));
        self.last_search = Some(Search {
            pattern: pattern.clone(),
            backward,
        });
        self.run_search(&pattern, backward, edits, commands);
    }

    /// Move to the count-th match of `pattern`. Search is an exclusive
    /// motion, so it composes with a pending operator (`d/foo<cr>`, `dn`).
    fn run_search(
        &mut self,
        pattern: &str,
        backward: bool,
        edits: &mut Vec<Edit>,
        commands: &mut Vec<Command>,
    ) {
        let count = self.pending.count();
        let target = match search::Pattern::parse(pattern) {
            Ok(p) => search::find(&self.buf, self.cursor, &p, backward, count),
            Err(msg) => {
                self.message = Some(msg);
                return self.clear_pending();
            }
        };
        match target {
            Some(pos) => {
                self.pending.awaiting = Awaiting::None;
                self.do_motion(pos, MotionKind::Exclusive, edits, commands);
            }
            None => {
                self.message = Some(format!("pattern not found: {pattern}"));
                self.clear_pending();
            }
        }
    }

    // ---- ex command line -----------------------------------------------------

    /// Collect the `:` command line until `<cr>` runs it; `<esc>`, and `<bs>`
    /// past the start of the line, abandon it like Vim.
    fn resolve_ex(&mut self, key: Key, edits: &mut Vec<Edit>) {
        match key {
            Key::Char(c) => self.pending.prompt.push(c),
            Key::Backspace => {
                if self.pending.prompt.pop().is_none() {
                    self.clear_pending();
                }
            }
            Key::Enter => {
                let line = std::mem::take(&mut self.pending.prompt);
                self.clear_pending();
                self.run_ex(&line, edits);
            }
            Key::Esc => self.clear_pending(),
            Key::Ctrl(_) => {}
        }
    }

    fn run_ex(&mut self, line: &str, edits: &mut Vec<Edit>) {
        // A selection in play becomes `'<`/`'>`, and running the command
        // leaves visual mode, as `:'<,'>s/…` does in Vim.
        if let Mode::Visual { .. } = self.mode {
            let (s, e) = ordered(self.anchor, self.cursor);
            self.last_visual = Some((s.line, e.line));
            self.mode = Mode::Normal;
            self.cursor = self.clamp_normal(s);
            self.desired_col = self.cursor.col;
        }
        let ctx = ex::Context {
            current: self.cursor.line,
            last: self.buf.last_line(),
            visual: self.last_visual,
        };
        match ex::parse(line, &ctx) {
            Ok(ex::Command::Nothing) => {}
            Ok(ex::Command::Goto(target)) => {
                let target = target.min(self.buf.last_line());
                self.cursor = Pos::new(target, self.buf.first_non_blank(target));
                self.desired_col = self.cursor.col;
            }
            Ok(ex::Command::Substitute(sub)) => self.substitute(&sub, edits),
            Err(msg) => self.message = Some(msg),
        }
    }

    /// Run `:s` over its line range: at most one edit per line, spanning that
    /// line's first match start to its last match end.
    fn substitute(&mut self, sub: &ex::Substitute, edits: &mut Vec<Edit>) {
        self.clear_pending();
        let last_used = self.last_search.as_ref().map(|s| s.pattern.clone());
        let Some(source) = sub.pattern.clone().or(last_used) else {
            self.message = Some("no previous regular expression".into());
            return;
        };
        let pattern = match search::Pattern::parse_case(&source, sub.ignore_case.unwrap_or(false)) {
            Ok(p) => p,
            Err(msg) => {
                self.message = Some(msg);
                return;
            }
        };
        let previous = self.last_replacement.clone().unwrap_or_default();
        let replacement = ex::expand_tilde(
            sub.replacement.as_deref().unwrap_or(&previous),
            &previous,
        );
        self.last_search = Some(Search { pattern: source.clone(), backward: false });
        if !sub.count_only {
            self.last_replacement = Some(replacement.clone());
        }

        let mut line_edits: Vec<Edit> = Vec::new();
        let (mut hits, mut lines) = (0usize, 0usize);
        for line in sub.first..=sub.last.min(self.buf.last_line()) {
            let text = self.buf.line(line);
            let found = pattern.find_all(text, sub.global);
            let (Some(first), Some(end)) = (found.first(), found.last().map(|m| m.end)) else {
                continue;
            };
            hits += found.len();
            lines += 1;
            if sub.count_only {
                continue;
            }
            let start = first.start;
            let mut out = String::new();
            let mut col = start;
            for m in &found {
                out.push_str(slice_cols(text, col, m.start));
                out.push_str(&ex::expand(&replacement, m));
                col = m.end;
            }
            line_edits.push(Edit {
                start: Pos::new(line, start),
                end: Pos::new(line, end),
                text: out,
            });
        }

        if hits == 0 {
            if !sub.quiet {
                self.message = Some(format!("pattern not found: {source}"));
            }
            return;
        }
        if sub.count_only {
            self.message = Some(format!(
                "{hits} match{} on {lines} line{}",
                if hits == 1 { "" } else { "es" },
                plural(lines)
            ));
            return;
        }
        // Bottom-up, so each edit's pre-state coordinates still hold when it
        // reaches the mirror. The host applies them all to one snapshot.
        for e in line_edits.iter().rev() {
            self.buf.apply_change(e.start, e.end, &e.text);
        }
        // Vim leaves the cursor on the last line it changed; a replacement
        // that broke lines pushes that line further down.
        let added: usize = line_edits.iter().map(|e| e.text.matches('\n').count()).sum();
        let target = line_edits.last().map_or(self.cursor.line, |e| e.start.line) + added;
        let target = target.min(self.buf.last_line());
        self.cursor = Pos::new(target, self.buf.first_non_blank(target));
        self.desired_col = self.cursor.col;
        edits.extend(line_edits);
        self.message = Some(format!(
            "{hits} substitution{} on {lines} line{}",
            plural(hits),
            plural(lines)
        ));
    }

    fn resolve_replace(&mut self, key: Key, edits: &mut Vec<Edit>) {
        let count = self.pending.count();
        let Key::Char(ch) = key else {
            return self.clear_pending();
        };
        if let Mode::Visual { linewise } = self.mode {
            self.visual_replace(ch, linewise, edits);
            return;
        }
        let cur = self.cursor;
        let cols = chars_with_cols(self.buf.line(cur.line));
        let idx = cols.partition_point(|&(c, _)| c <= cur.col).saturating_sub(1);
        if cols.is_empty() || idx + count > cols.len() {
            return self.clear_pending(); // not enough chars: r fails
        }
        let end = cols
            .get(idx + count)
            .map_or(Pos::new(cur.line, self.buf.line_len(cur.line)), |&(c, _)| {
                Pos::new(cur.line, c)
            });
        let replacement: String = std::iter::repeat_n(ch, count).collect();
        self.emit_edit(Pos::new(cur.line, cols[idx].0), end, &replacement, edits);
        self.cursor = self.clamp_normal(Pos::new(
            cur.line,
            cols[idx].0 + ch.len_utf16() * (count - 1),
        ));
        self.desired_col = self.cursor.col;
        self.clear_pending();
    }

    fn visual_replace(&mut self, ch: char, linewise: bool, edits: &mut Vec<Edit>) {
        let (s, e) = self.visual_range(linewise);
        for line in s.line..=e.line.min(self.buf.last_line()) {
            let len = self.buf.line_len(line);
            let from = if line == s.line && !linewise { s.col } else { 0 };
            let to = if line == e.line && !linewise { e.col.min(len) } else { len };
            if from >= to {
                continue;
            }
            let n = self.buf.slice(Pos::new(line, from), Pos::new(line, to)).chars().count();
            let replacement: String = std::iter::repeat_n(ch, n).collect();
            edits.push(Edit {
                start: Pos::new(line, from),
                end: Pos::new(line, to),
                text: replacement,
            });
        }
        for e in edits.iter() {
            self.buf.apply_change(e.start, e.end, &e.text);
        }
        self.mode = Mode::Normal;
        self.cursor = self.clamp_normal(s);
        self.desired_col = self.cursor.col;
        self.clear_pending();
    }

    fn resolve_g(&mut self, key: Key, edits: &mut Vec<Edit>, commands: &mut Vec<Command>) {
        match key {
            Key::Char('g') => {
                let line = if self.pending.has_count() {
                    (self.pending.count() - 1).min(self.buf.last_line())
                } else {
                    0
                };
                let target = Pos::new(line, self.buf.first_non_blank(line));
                self.pending.awaiting = Awaiting::None;
                self.do_motion(target, MotionKind::Linewise, edits, commands);
            }
            _ => self.clear_pending(),
        }
    }

    fn resolve_z(&mut self, key: Key, commands: &mut Vec<Command>) {
        match key {
            Key::Char('z') => commands.push(Command::Scroll { to: "center" }),
            Key::Char('t') => commands.push(Command::Scroll { to: "top" }),
            Key::Char('b') => commands.push(Command::Scroll { to: "bottom" }),
            _ => {}
        }
        self.clear_pending();
    }

    fn resolve_object(
        &mut self,
        around: bool,
        key: Key,
        edits: &mut Vec<Edit>,
        commands: &mut Vec<Command>,
    ) {
        let Key::Char(c) = key else {
            return self.clear_pending();
        };
        let Some(obj) = TextObject::parse(c) else {
            return self.clear_pending();
        };
        let Some((s, e)) = textobj::range(&self.buf, self.cursor, obj, around) else {
            return self.clear_pending();
        };
        if let Some(op) = self.pending.op {
            self.apply_operator_charwise(op, s, e, edits, commands);
            self.clear_pending();
        } else if matches!(self.mode, Mode::Visual { .. }) {
            self.mode = Mode::Visual { linewise: false };
            self.anchor = s;
            self.cursor = self.clamp_normal(char_before(&self.buf, e));
            self.desired_col = self.cursor.col;
            self.clear_pending();
        } else {
            self.clear_pending();
        }
    }

    // ---- operators ----------------------------------------------------------

    fn doubled_operator(&mut self, op: Op, edits: &mut Vec<Edit>, commands: &mut Vec<Command>) {
        let count = self.pending.count();
        let l1 = self.cursor.line;
        let l2 = (l1 + count - 1).min(self.buf.last_line());
        self.apply_operator_linewise(op, l1, l2, edits, commands);
        self.clear_pending();
    }

    fn visual_operator(&mut self, op: Op, edits: &mut Vec<Edit>, commands: &mut Vec<Command>) {
        let Mode::Visual { linewise } = self.mode else {
            return;
        };
        self.mode = Mode::Normal;
        if linewise || matches!(op, Op::Indent | Op::Dedent) {
            let (s, e) = self.visual_range(true);
            self.apply_operator_linewise(op, s.line, e.line, edits, commands);
        } else {
            let (s, e) = self.visual_range(false);
            self.apply_operator_charwise(op, s, e, edits, commands);
        }
        self.clear_pending();
    }

    /// Visual range. Charwise: `[start, end)` with `end` one past the cursor
    /// char. Linewise: positions carry the line span only.
    fn visual_range(&self, linewise: bool) -> (Pos, Pos) {
        let (s, e) = ordered(self.anchor, self.cursor);
        if linewise {
            (Pos::new(s.line, 0), Pos::new(e.line, self.buf.line_len(e.line)))
        } else {
            (s, motion::right(&self.buf, e, 1))
        }
    }

    fn apply_operator_charwise(
        &mut self,
        op: Op,
        s: Pos,
        e: Pos,
        edits: &mut Vec<Edit>,
        commands: &mut Vec<Command>,
    ) {
        match op {
            Op::Delete => {
                self.yank_range(s, e, false);
                self.emit_edit(s, e, "", edits);
                self.cursor = self.clamp_normal(s);
            }
            Op::Change => {
                self.yank_range(s, e, false);
                self.emit_edit(s, e, "", edits);
                self.enter_insert(s);
            }
            Op::Yank => {
                self.yank_range(s, e, false);
                if s < self.cursor {
                    self.cursor = self.clamp_normal(s);
                }
            }
            Op::Indent | Op::Dedent => {
                commands.push(Command::IndentLines {
                    start_line: s.line,
                    end_line: e.line,
                    dedent: op == Op::Dedent,
                });
                self.cursor = self.clamp_normal(Pos::new(s.line, self.cursor.col));
            }
        }
        self.desired_col = self.cursor.col;
    }

    fn apply_operator_linewise(
        &mut self,
        op: Op,
        l1: usize,
        l2: usize,
        edits: &mut Vec<Edit>,
        commands: &mut Vec<Command>,
    ) {
        let l2 = l2.min(self.buf.last_line());
        match op {
            Op::Delete => {
                self.yank_lines(l1, l2);
                self.delete_lines(l1, l2, edits);
            }
            Op::Change => {
                self.yank_lines(l1, l2);
                self.linewise_change(l1, l2 - l1 + 1, edits);
            }
            Op::Yank => {
                self.yank_lines(l1, l2);
                if l1 < self.cursor.line {
                    self.cursor = self.clamp_normal(Pos::new(l1, self.cursor.col));
                }
            }
            Op::Indent | Op::Dedent => {
                commands.push(Command::IndentLines {
                    start_line: l1,
                    end_line: l2,
                    dedent: op == Op::Dedent,
                });
                self.cursor = self.clamp_normal(Pos::new(l1, self.cursor.col));
            }
        }
        self.desired_col = self.cursor.col;
    }

    fn yank_range(&mut self, s: Pos, e: Pos, linewise: bool) {
        self.register = Register {
            text: self.buf.slice(s, e),
            linewise,
        };
    }

    fn yank_lines(&mut self, l1: usize, l2: usize) {
        let text = (l1..=l2)
            .map(|l| self.buf.line(l))
            .collect::<Vec<_>>()
            .join("\n");
        self.register = Register { text, linewise: true };
    }

    fn linewise_yank(&mut self, count: usize) {
        let l1 = self.cursor.line;
        let l2 = (l1 + count - 1).min(self.buf.last_line());
        self.yank_lines(l1, l2);
        self.clear_pending();
    }

    /// Delete whole lines including one adjoining newline.
    fn delete_lines(&mut self, l1: usize, l2: usize, edits: &mut Vec<Edit>) {
        let last = self.buf.last_line();
        let (s, e) = if l2 < last {
            (Pos::new(l1, 0), Pos::new(l2 + 1, 0))
        } else if l1 > 0 {
            (
                Pos::new(l1 - 1, self.buf.line_len(l1 - 1)),
                Pos::new(l2, self.buf.line_len(l2)),
            )
        } else {
            (Pos::new(0, 0), Pos::new(l2, self.buf.line_len(l2)))
        };
        self.emit_edit(s, e, "", edits);
        let line = l1.min(self.buf.last_line());
        self.cursor = Pos::new(line, self.buf.first_non_blank(line));
        self.desired_col = self.cursor.col;
    }

    /// `cc`/`S`: blank the lines (keeping one) and enter insert.
    fn linewise_change(&mut self, l1: usize, count: usize, edits: &mut Vec<Edit>) {
        let l2 = (l1 + count - 1).min(self.buf.last_line());
        self.yank_lines(l1, l2);
        self.emit_edit(Pos::new(l1, 0), Pos::new(l2, self.buf.line_len(l2)), "", edits);
        self.clear_pending();
        self.enter_insert(Pos::new(l1, 0));
    }

    // ---- standalone edit actions ---------------------------------------------

    fn emit_edit(&mut self, s: Pos, e: Pos, text: &str, edits: &mut Vec<Edit>) {
        edits.push(Edit {
            start: s,
            end: e,
            text: text.to_string(),
        });
        self.buf.apply_change(s, e, text);
    }

    /// `x` / `s`: delete `count` chars under/after the cursor.
    fn delete_chars(&mut self, count: usize, edits: &mut Vec<Edit>, then_insert: bool) {
        let cur = self.cursor;
        if self.buf.line_len(cur.line) == 0 {
            return self.clear_pending();
        }
        let e = motion::right(&self.buf, cur, count);
        self.yank_range(cur, e, false);
        self.emit_edit(cur, e, "", edits);
        self.clear_pending();
        if then_insert {
            self.enter_insert(cur);
        } else {
            self.cursor = self.clamp_normal(cur);
            self.desired_col = self.cursor.col;
        }
    }

    /// `X`: delete `count` chars before the cursor (line-local).
    fn delete_back(&mut self, count: usize, edits: &mut Vec<Edit>) {
        let cur = self.cursor;
        let cols = chars_with_cols(self.buf.line(cur.line));
        let idx = cols.partition_point(|&(c, _)| c < cur.col);
        let from = idx.saturating_sub(count);
        if idx == 0 {
            return self.clear_pending();
        }
        let s = Pos::new(cur.line, cols[from].0);
        let e = Pos::new(cur.line, cur.col);
        self.yank_range(s, e, false);
        self.emit_edit(s, e, "", edits);
        self.cursor = self.clamp_normal(s);
        self.desired_col = self.cursor.col;
        self.clear_pending();
    }

    /// `D` / `C`.
    fn delete_to_eol(&mut self, edits: &mut Vec<Edit>, then_insert: bool) {
        let cur = self.cursor;
        let e = Pos::new(cur.line, self.buf.line_len(cur.line));
        self.yank_range(cur, e, false);
        self.emit_edit(cur, e, "", edits);
        self.clear_pending();
        if then_insert {
            self.enter_insert(cur);
        } else {
            self.cursor = self.clamp_normal(cur);
            self.desired_col = self.cursor.col;
        }
    }

    /// `~`: toggle case of `count` chars, cursor lands after the last one.
    fn toggle_case(&mut self, count: usize, edits: &mut Vec<Edit>) {
        let cur = self.cursor;
        let e = motion::right(&self.buf, cur, count);
        if e == cur {
            return self.clear_pending();
        }
        let toggled: String = self
            .buf
            .slice(cur, e)
            .chars()
            .map(|ch| {
                if ch.is_uppercase() {
                    ch.to_lowercase().collect::<String>()
                } else {
                    ch.to_uppercase().collect::<String>()
                }
            })
            .collect();
        self.emit_edit(cur, e, &toggled, edits);
        self.cursor = self.clamp_normal(e);
        self.desired_col = self.cursor.col;
        self.clear_pending();
    }

    fn visual_toggle_case(&mut self, edits: &mut Vec<Edit>) {
        let Mode::Visual { linewise } = self.mode else {
            return;
        };
        let (s, e) = self.visual_range(linewise);
        let mut line_edits = Vec::new();
        for line in s.line..=e.line.min(self.buf.last_line()) {
            let len = self.buf.line_len(line);
            let from = if line == s.line && !linewise { s.col } else { 0 };
            let to = if line == e.line && !linewise { e.col.min(len) } else { len };
            if from >= to {
                continue;
            }
            let text = self.buf.slice(Pos::new(line, from), Pos::new(line, to));
            let toggled: String = text
                .chars()
                .map(|ch| {
                    if ch.is_uppercase() {
                        ch.to_lowercase().collect::<String>()
                    } else {
                        ch.to_uppercase().collect::<String>()
                    }
                })
                .collect();
            line_edits.push(Edit {
                start: Pos::new(line, from),
                end: Pos::new(line, to),
                text: toggled,
            });
        }
        for e in &line_edits {
            self.buf.apply_change(e.start, e.end, &e.text);
        }
        edits.extend(line_edits);
        self.mode = Mode::Normal;
        self.cursor = self.clamp_normal(s);
        self.desired_col = self.cursor.col;
        self.clear_pending();
    }

    /// `J`: join lines with a single space, collapsing leading whitespace.
    fn join_lines(&mut self, count: usize, edits: &mut Vec<Edit>) {
        // In visual mode J joins the selected lines.
        let (l1, joins) = if let Mode::Visual { .. } = self.mode {
            let (s, e) = ordered(self.anchor, self.cursor);
            self.mode = Mode::Normal;
            (s.line, (e.line - s.line).max(1))
        } else {
            (self.cursor.line, count.max(2) - 1)
        };
        let last_join = (l1 + joins).min(self.buf.last_line());
        if last_join == l1 {
            return self.clear_pending();
        }
        let mut text = self.buf.line(l1).to_string();
        let mut cursor_col = utf16_len(&text);
        for line in l1 + 1..=last_join {
            let next = self.buf.line(line).trim_start();
            cursor_col = utf16_len(&text);
            if next.is_empty() {
                continue;
            }
            if !text.is_empty() && !text.ends_with(' ') && !next.starts_with(')') {
                text.push(' ');
            }
            text.push_str(next);
        }
        let end = Pos::new(last_join, self.buf.line_len(last_join));
        self.emit_edit(Pos::new(l1, 0), end, &text, edits);
        self.cursor = self.clamp_normal(Pos::new(l1, cursor_col));
        self.desired_col = self.cursor.col;
        self.clear_pending();
    }

    /// `p` / `P`.
    fn paste(&mut self, before: bool, count: usize, edits: &mut Vec<Edit>) {
        // Visual paste: delete the selection, then paste at the gap.
        if let Mode::Visual { linewise } = self.mode {
            let saved = self.register.clone();
            let (s, e) = self.visual_range(linewise);
            self.mode = Mode::Normal;
            if linewise {
                self.yank_lines(s.line, e.line);
                self.delete_lines(s.line, e.line, edits);
            } else {
                self.yank_range(s, e, false);
                self.emit_edit(s, e, "", edits);
                self.cursor = self.clamp_normal(s);
            }
            let deleted = std::mem::replace(&mut self.register, saved);
            if self.register.linewise || linewise {
                self.paste_at(true, count, edits);
            } else {
                // Insert exactly at the gap (which may sit at end of line,
                // past where a normal-mode cursor can rest).
                self.paste_charwise_at(s, count, edits);
            }
            self.register = deleted;
            self.clear_pending();
            return;
        }
        self.paste_at(before, count, edits);
        self.clear_pending();
    }

    fn paste_at(&mut self, before: bool, count: usize, edits: &mut Vec<Edit>) {
        let reg = self.register.clone();
        if reg.text.is_empty() {
            return;
        }
        let cur = self.cursor;
        if reg.linewise {
            let unit = format!("{}\n", reg.text);
            let block = unit.repeat(count.max(1));
            if before {
                self.emit_edit(Pos::new(cur.line, 0), Pos::new(cur.line, 0), &block, edits);
                self.cursor = Pos::new(cur.line, self.buf.first_non_blank(cur.line));
            } else if cur.line < self.buf.last_line() {
                let at = Pos::new(cur.line + 1, 0);
                self.emit_edit(at, at, &block, edits);
                self.cursor = Pos::new(cur.line + 1, self.buf.first_non_blank(cur.line + 1));
            } else {
                let at = Pos::new(cur.line, self.buf.line_len(cur.line));
                let text = format!("\n{}", &block[..block.len() - 1]);
                self.emit_edit(at, at, &text, edits);
                let line = (cur.line + 1).min(self.buf.last_line());
                self.cursor = Pos::new(line, self.buf.first_non_blank(line));
            }
        } else {
            let at = if before {
                cur
            } else {
                motion::right(&self.buf, cur, 1)
            };
            self.paste_charwise_at(at, count, edits);
        }
        self.desired_col = self.cursor.col;
    }

    fn paste_charwise_at(&mut self, at: Pos, count: usize, edits: &mut Vec<Edit>) {
        let text = self.register.text.repeat(count.max(1));
        if text.is_empty() {
            return;
        }
        self.emit_edit(at, at, &text, edits);
        // Cursor lands on the last pasted char.
        let newlines = text.matches('\n').count();
        let last_seg = text.rsplit('\n').next().unwrap_or("");
        let end_line = at.line + newlines;
        let end_col = if newlines == 0 {
            at.col + utf16_len(last_seg)
        } else {
            utf16_len(last_seg)
        };
        self.cursor = self.clamp_normal(char_before(&self.buf, Pos::new(end_line, end_col)));
        self.desired_col = self.cursor.col;
    }
}

fn ordered(a: Pos, b: Pos) -> (Pos, Pos) {
    if a <= b { (a, b) } else { (b, a) }
}

/// Text of one line between two UTF-16 columns.
fn slice_cols(line: &str, from: usize, to: usize) -> &str {
    let a = utf16_to_byte(line, from);
    let b = utf16_to_byte(line, to);
    &line[a..b.max(a)]
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}
