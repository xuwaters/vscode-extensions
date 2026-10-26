//! Registers (`:help registers`): where yanks and deletes go, and what puts
//! read.
//!
//! Vim keeps one set for the whole editor — a `"ayy` in one buffer pastes
//! in another — and every session in the wasm instance is a buffer of the
//! same editor, so the store lives beside the sessions, not in one. The
//! rules for which registers a yank or a delete fills are register.c's
//! `op_yank` and ops.c's `op_delete`; see `store`.
//!
//! Three registers are not here. `"/` is the session's last search pattern
//! and `"_` holds nothing, so state.rs answers for both; `"+` / `"*` are
//! the system clipboard, which only the host can reach, so the slot here
//! holds what the host last supplied and what the engine last wrote — the
//! protocol is described on `Command::SetClipboard`.

use std::cell::RefCell;

use serde::Serialize;

/// What a register holds.
#[derive(Clone, Default, Debug, PartialEq, Eq, Serialize)]
pub struct Register {
    /// The text, lines joined with `\n`. Linewise text has no final line
    /// break: `yy` on `foo` holds `foo`.
    pub text: String,
    pub linewise: bool,
    /// One piece per cursor, top to bottom, when a key at several cursors
    /// filled the register; `text` is then the pieces joined with line
    /// breaks. A put at the same number of cursors gives each its own
    /// piece; any other put puts `text`. Empty for a single cursor's write.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<String>,
}

impl Register {
    pub fn new(text: impl Into<String>, linewise: bool) -> Register {
        Register { text: text.into(), linewise, parts: Vec::new() }
    }

    /// The text a put at cursor `slot` — its place in document order, and
    /// how many cursors are putting — puts.
    pub fn part(&self, slot: Option<(usize, usize)>) -> &str {
        match slot {
            Some((i, n)) if n > 1 && self.parts.len() == n => &self.parts[i],
            _ => &self.text,
        }
    }

    /// `"Ayy`: `more` goes after what is there (register.c `op_yank`).
    /// Lines win: a linewise yank turns the register linewise, and new text
    /// after linewise text starts a line of its own. Charwise after
    /// charwise runs on, the first new line joining the last old one.
    /// Pieces stay pieces when both sides have as many.
    fn append(&mut self, more: Register) {
        let sep = if self.linewise || more.linewise { "\n" } else { "" };
        if self.parts.len() > 1 && self.parts.len() == more.parts.len() {
            for (part, extra) in self.parts.iter_mut().zip(&more.parts) {
                part.push_str(sep);
                part.push_str(extra);
            }
        } else {
            self.parts.clear();
        }
        self.text.push_str(sep);
        self.text.push_str(&more.text);
        self.linewise |= more.linewise;
    }
}

/// Why text is going into the registers, which decides which ones get it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Yank,
    /// A delete or change: of whole lines or across lines (`lines`), or made
    /// with one of the motions `:help quote_number` lists (`reg_one`).
    Delete { lines: bool, reg_one: bool },
}

/// Slots, in Vim's own order: `"0`–`"9`, `"a`–`"z`, then the rest.
const NAMED: usize = 10;
const SMALL: usize = 36;
const CLIPBOARD: usize = 37;
const INSERTED: usize = 38;
const COMMAND: usize = 39;
const SLOTS: usize = 40;

#[derive(Debug)]
struct Store {
    regs: Vec<Option<Register>>,
    /// The slot `""` reads — Vim's `y_previous`, the register last written.
    unnamed: usize,
    /// The clipboard slot was written since the host last heard about it.
    clipboard_written: bool,
}

impl Default for Store {
    fn default() -> Store {
        Store { regs: vec![None; SLOTS], unnamed: 0, clipboard_written: false }
    }
}

thread_local! {
    static STORE: RefCell<Store> = RefCell::default();
}

fn slot(name: char) -> Option<usize> {
    match name {
        '0'..='9' => Some(name as usize - '0' as usize),
        'a'..='z' => Some(NAMED + (name as usize - 'a' as usize)),
        'A'..='Z' => Some(NAMED + (name as usize - 'A' as usize)),
        '-' => Some(SMALL),
        '+' | '*' => Some(CLIPBOARD),
        '.' => Some(INSERTED),
        ':' => Some(COMMAND),
        _ => None,
    }
}

/// Whether `"{name}` names a register a put can read: everything a yank can
/// fill, plus the read-only `". ": "/`.
pub fn readable(name: char) -> bool {
    matches!(name, '"' | '_' | '/') || slot(name).is_some()
}

/// Whether `"{name}` names a register a yank or delete can fill. The
/// read-only ones refuse, and the command they were typed for does
/// nothing, as Vim's do.
pub fn writable(name: char) -> bool {
    matches!(name, '"' | '_') || slot(name).is_some_and(|i| i < INSERTED)
}

/// Whether `name` is the system clipboard, which the host has to supply.
pub fn is_clipboard(name: char) -> bool {
    matches!(name, '+' | '*')
}

/// The register `name` reads, or `None` when it is empty. `"` is whichever
/// was written last; an uppercase name reads its lowercase register.
pub fn get(name: char) -> Option<Register> {
    STORE.with(|s| {
        let s = s.borrow();
        let i = if name == '"' { s.unnamed } else { slot(name)? };
        s.regs[i].clone().filter(|r| !r.text.is_empty() || r.linewise)
    })
}

/// File what an operator took (register.c `op_yank`, ops.c `op_delete`).
/// `name` is the register typed in front of it, `None` for none.
///
/// - `"_` keeps everything as it was.
/// - A yank goes to the named register, or to `"0` without one.
/// - A delete goes to the named register too, and one of lines (or with a
///   `reg_one` motion, see `Kind`) also to `"1`, pushing `"1`–`"8` down to
///   `"2`–`"9`. Without a name, one within a line goes to `"-` as well.
/// - An uppercase name appends to its register.
///
/// `""` then reads whatever was written last, except that an append leaves
/// it on the register appended to.
pub fn store(name: Option<char>, reg: Register, kind: Kind) {
    let name = name.filter(|&c| c != '"');
    if name == Some('_') || name.is_some_and(|c| !writable(c)) {
        return;
    }
    STORE.with(|s| {
        let mut s = s.borrow_mut();
        let mut appended = false;
        if let Some(c) = name {
            appended = c.is_ascii_uppercase();
            s.write(c, reg.clone());
        }
        match kind {
            Kind::Yank if name.is_none() => s.write('0', reg),
            Kind::Yank => {}
            Kind::Delete { lines, reg_one } => {
                if lines || reg_one {
                    s.regs[1..=9].rotate_right(1);
                    s.regs[1] = Some(reg.clone());
                    if !appended {
                        s.unnamed = 1;
                    }
                }
                if name.is_none() && !lines {
                    s.write('-', reg);
                }
            }
        }
    });
}

/// Write `name` the way a recording or `:let @a = ...` does: an uppercase
/// name appends, nothing else is touched, and `""` stays where it pointed.
/// False for a name that cannot be written (`"_`, `".`, `":`, and `"/`,
/// which state.rs handles itself). `"` writes `"0` and points `""` at it.
pub fn set(name: char, reg: Register) -> bool {
    if name == '"' {
        STORE.with(|s| s.borrow_mut().write('0', reg));
        return true;
    }
    if name == '_' || !writable(name) {
        return false;
    }
    STORE.with(|s| {
        let mut s = s.borrow_mut();
        let unnamed = s.unnamed;
        s.write(name, reg);
        s.unnamed = unnamed;
    });
    true
}

/// The host's clipboard text, read just before a key that puts from `"+`
/// or `"*`. Text ending in a line break is linewise, the way Vim reads a
/// selection another program made; Windows line breaks become `\n`.
pub fn set_clipboard(text: &str) {
    let text = text.replace("\r\n", "\n");
    let reg = match text.strip_suffix('\n') {
        Some(lines) => Register::new(lines, true),
        None => Register::new(text, false),
    };
    STORE.with(|s| s.borrow_mut().regs[CLIPBOARD] = Some(reg));
}

/// What to put on the system clipboard, once per write into `"+` / `"*`:
/// the text, with the final line break that marks it linewise for whoever
/// reads it next.
pub fn take_clipboard() -> Option<String> {
    STORE.with(|s| {
        let mut s = s.borrow_mut();
        if !std::mem::take(&mut s.clipboard_written) {
            return None;
        }
        let reg = s.regs[CLIPBOARD].clone().unwrap_or_default();
        Some(if reg.linewise { format!("{}\n", reg.text) } else { reg.text })
    })
}

/// `".`: the text the last insert typed.
pub fn set_inserted(text: String) {
    STORE.with(|s| s.borrow_mut().regs[INSERTED] = Some(Register::new(text, false)));
}

/// `":`: the last command line run.
pub fn set_command(text: &str) {
    STORE.with(|s| s.borrow_mut().regs[COMMAND] = Some(Register::new(text, false)));
}

/// Every register with something in it, in `:registers` order: `""`, the
/// numbered ones, the named ones, `"-`, the clipboard, `".` and `":`.
pub fn listing() -> Vec<(char, Register)> {
    let names = std::iter::once('"')
        .chain('0'..='9')
        .chain('a'..='z')
        .chain(['-', '+', '.', ':']);
    names.filter_map(|c| get(c).map(|r| (c, r))).collect()
}

impl Store {
    /// Fill (or, for an uppercase name, append to) one register and make it
    /// the one `""` reads.
    fn write(&mut self, name: char, reg: Register) {
        let Some(i) = slot(name) else { return };
        match &mut self.regs[i] {
            Some(old) if name.is_ascii_uppercase() && (!old.text.is_empty() || old.linewise) => {
                old.append(reg)
            }
            other => *other = Some(reg),
        }
        self.unnamed = i;
        if i == CLIPBOARD {
            self.clipboard_written = true;
        }
    }
}
