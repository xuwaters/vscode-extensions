//! Macros (`:help recording`): `q{0-9a-zA-Z"}` records what is typed into a
//! register, `q` stops, and `@{reg}` runs a register's text as keys.
//!
//! A recording belongs to the editor, not to one document — keys typed in
//! another buffer while it runs go into it, as Vim's do — so, like the
//! registers it fills, it lives beside the sessions rather than in one. So
//! does the register `@@` runs again.
//!
//! The engine records and the host runs. Recording is the engine's because
//! it is the one place every key goes through — every key but the ones typed
//! in insert mode, which the editor types natively. What those leave behind
//! does reach the engine (state.rs `InsertRecord`), and that text goes into
//! the recording in front of the `<esc>` that ends the insert: the text the
//! insert put in rather than the keys that typed it, so a backspace, an
//! accepted completion or an auto-closed bracket replay as what they did.
//! Running is the host's (see `Command::Execute`): a key may have to wait for
//! an edit, a line-insert or the clipboard, all of which are the host's.

use std::cell::RefCell;

use crate::keys::{self, Key};

#[derive(Default)]
struct State {
    /// The register being recorded into and what has gone in so far.
    recording: Option<(char, String)>,
    /// The register `@` last ran, for `@@`.
    last_run: Option<char>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::default();
}

/// Whether `q{name}` can record into `name`: the numbered and named
/// registers, and `"`. An uppercase name appends to its register.
pub fn recordable(name: char) -> bool {
    name.is_ascii_alphanumeric() || name == '"'
}

/// The register a recording is going into, if one is.
pub fn recording() -> Option<char> {
    STATE.with(|s| s.borrow().recording.as_ref().map(|&(name, _)| name))
}

pub fn start(name: char) {
    STATE.with(|s| s.borrow_mut().recording = Some((name, String::new())));
}

/// End the recording: the register it was for and what it recorded, which
/// goes into the register only now — `"ap` while recording puts what `a`
/// held before.
pub fn stop() -> Option<(char, String)> {
    STATE.with(|s| s.borrow_mut().recording.take())
}

/// Add keys, already spelled (`Key::notation`), to the recording in
/// progress; nothing when there is none.
pub fn push(spelled: &str) {
    STATE.with(|s| {
        if let Some((_, keys)) = &mut s.borrow_mut().recording {
            keys.push_str(spelled);
        }
    });
}

pub fn last_run() -> Option<char> {
    STATE.with(|s| s.borrow().last_run)
}

pub fn set_last_run(name: char) {
    STATE.with(|s| s.borrow_mut().last_run = Some(name));
}

/// Text an insert typed, as it goes into a recording.
pub fn spell(text: &str) -> String {
    text.chars().map(|c| Key::Char(c).notation()).collect()
}

/// The keys a register's text stands for, as `@` runs them: `<>` notation
/// as a recording writes it (`<esc>`, `<cr>`, `<lt>`), and the raw control
/// characters a register written some other way may hold — a real escape is
/// `<esc>`, a line break `<cr>`. A linewise register runs every line with
/// the line break after it, the last one included (register.c
/// `do_execreg`), so a line yanked with `yy` ends on a `<cr>`.
pub fn keys(text: &str, linewise: bool) -> Vec<Key> {
    let mut keys = keys::parse_sequence(text);
    if linewise {
        keys.push(Key::Enter);
    }
    keys.into_iter()
        .map(|key| match key {
            Key::Char('\n' | '\r') => Key::Enter,
            Key::Char('\u{1b}') => Key::Esc,
            Key::Char('\u{8}' | '\u{7f}') => Key::Backspace,
            Key::Char(c) if c != '\t' && (c as u32) < 0x20 => Key::Ctrl((c as u8 + b'`') as char),
            key => key,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_read_as_keys_in_either_spelling() {
        assert_eq!(
            keys("ix<esc>", false),
            [Key::Char('i'), Key::Char('x'), Key::Esc]
        );
        assert_eq!(
            keys("ix\u{1b}j\u{1}", false),
            [Key::Char('i'), Key::Char('x'), Key::Esc, Key::Char('j'), Key::Ctrl('a')]
        );
        assert_eq!(keys("dd\nx", true), [Key::Char('d'), Key::Char('d'), Key::Enter, Key::Char('x'), Key::Enter]);
        assert_eq!(spell("a<b\n"), "a<lt>b<cr>");
    }
}
