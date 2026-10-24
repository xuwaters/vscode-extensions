//! Marks and the jumplist (`:help mark-motions`, `:help jump-motions`).
//!
//! A mark is a position that travels with the text it was set on. Every
//! edit, whoever made it, is worked out once as a `Shift` against the text
//! it was made on, and each mark is carried through that — mark.c's
//! `mark_adjust`, which Vim calls with the lines an edit added or took away.
//! Like Vim's, a mark follows its line up and down, keeps its column through
//! an edit inside the line, and moves with the text when a line is split or
//! joined. When its line is deleted the mark goes with it, except for the
//! ones Vim only moves out of the way (`one_adjust_nodel`): the jumplist,
//! `'[ ']`, `'< '>` and the file marks.
//!
//! Buffer-local marks live in the session (state.rs); the uppercase file
//! marks are the editor's, so they live here beside the sessions, keyed by
//! the document they were set in.

use std::cell::RefCell;

use crate::buffer::{Buffer, Pos, utf16_len};

/// What one edit does to the lines around it.
#[derive(Clone, Debug)]
pub struct Shift {
    start: Pos,
    end: Pos,
    /// Where the edit's text ends once it is in.
    new_end: Pos,
    /// Lines the edit removed whole, inclusive, numbered as before it.
    gone: Option<(usize, usize)>,
    /// The edit stays inside one line: Vim leaves the line's marks alone.
    inline: bool,
}

impl Shift {
    /// The shift of replacing `[start, end)` with `text`, worked out on
    /// `buf` as it was before the edit.
    pub fn new(buf: &Buffer, start: Pos, end: Pos, text: &str) -> Shift {
        let breaks = text.matches('\n').count();
        let tail = utf16_len(text.rsplit('\n').next().unwrap_or(""));
        let new_end = if breaks == 0 {
            Pos::new(start.line, start.col + tail)
        } else {
            Pos::new(start.line + breaks, tail)
        };
        let inline = start.line == end.line && breaks == 0;
        // Which lines went whole. An edit from the start of a line to the
        // start of a later one takes the lines in between it and leaves the
        // last one as it was — `dd` and `:d` above the last line. One from the
        // end of a line to the end of a later one takes the lines after the
        // first — `dd` on the last line. Any other edit across lines keeps
        // the line it starts on, takes the ones strictly inside, and joins
        // what is left of the last one onto the first.
        let gone = if start.line >= end.line {
            None
        } else if start.col == 0 && end.col == 0 {
            Some((start.line, end.line - 1))
        } else if start.col >= buf.line_len(start.line) && end.col >= buf.line_len(end.line) {
            Some((start.line + 1, end.line))
        } else if end.line > start.line + 1 {
            Some((start.line + 1, end.line - 1))
        } else {
            None
        };
        Shift { start, end, new_end, gone, inline }
    }

    /// The same edit, told which lines it deletes. `dd` knows; the edit alone
    /// can be read two ways when the lines on either side of it are empty.
    pub fn removing(mut self, first: usize, last: usize) -> Shift {
        self.gone = Some((first, last));
        self.inline = false;
        self
    }

    /// An edit inside one line, which moves no mark at all.
    pub fn is_inline(&self) -> bool {
        self.inline
    }

    /// Where a mark ends up, or `None` when its line was deleted (mark.c's
    /// `one_adjust`): the lowercase marks, `''`, `'.` and `'^`.
    pub fn mark(&self, p: Pos) -> Option<Pos> {
        if self.inline {
            return Some(p);
        }
        if self.gone.is_some_and(|(a, b)| (a..=b).contains(&p.line)) {
            return None;
        }
        if p < self.start || p.line == self.start.line && p < self.end {
            return Some(p);
        }
        if p < self.end {
            // On the last line, in the part that was taken: it joins the
            // first line where the new text ends.
            return Some(self.new_end);
        }
        Some(if p.line == self.end.line {
            Pos::new(self.new_end.line, self.new_end.col + (p.col - self.end.col))
        } else {
            Pos::new(p.line - self.end.line + self.new_end.line, p.col)
        })
    }

    /// Like `mark`, but a mark on a deleted line moves to the line that took
    /// its place instead of going (`one_adjust_nodel`). Positions past the
    /// end of the buffer are the reader's to clamp.
    pub fn kept(&self, p: Pos) -> Pos {
        match self.gone {
            Some((a, b)) if !self.inline && (a..=b).contains(&p.line) => Pos::new(a, 0),
            _ => self.mark(p).unwrap_or(p),
        }
    }
}

/// Vim's `JUMPLISTSIZE`.
const JUMPLIST_SIZE: usize = 100;

/// The jumps that set `''`, oldest first, and where `ctrl-o` / `ctrl-i`
/// stand in them (mark.c `setpcmark`, `movemark`, `cleanup_jumplist`).
#[derive(Clone, Default, Debug)]
pub struct Jumplist {
    entries: Vec<Pos>,
    /// `entries.len()` while not walking the list.
    idx: usize,
}

impl Jumplist {
    /// A jump left `from`: it goes on the end, the oldest entry falls off a
    /// full list, and walking starts over.
    pub fn push(&mut self, from: Pos) {
        if self.entries.len() == JUMPLIST_SIZE {
            self.entries.remove(0);
        }
        self.entries.push(from);
        self.idx = self.entries.len();
    }

    /// Drop every entry a later one shares a line with, keeping the most
    /// recent visit — Vim's default (no `jumpoptions=stack`), done before
    /// walking the list.
    fn dedupe(&mut self) {
        let mut kept = Vec::with_capacity(self.entries.len());
        let mut idx = None;
        for (from, &p) in self.entries.iter().enumerate() {
            if self.idx == from {
                idx = Some(kept.len());
            }
            if !self.entries[from + 1..].iter().any(|q| q.line == p.line) {
                kept.push(p);
            }
        }
        self.idx = idx.unwrap_or(kept.len());
        self.entries = kept;
    }

    /// Carry every entry through an edit; a deleted line's entries move to
    /// the line that took its place.
    pub fn shift(&mut self, shift: &Shift) {
        for p in &mut self.entries {
            *p = shift.kept(*p);
        }
    }

    /// `:jumps`: every entry, oldest first, with how far `ctrl-o` / `ctrl-i`
    /// it is from where the list stands, and that place.
    pub fn listing(&mut self) -> (Vec<(usize, Pos)>, usize) {
        self.dedupe();
        let idx = self.idx;
        let rows = self.entries.iter().enumerate().map(|(i, &p)| (i.abs_diff(idx), p)).collect();
        (rows, idx)
    }
}

/// The buffer-local marks, `'^` and `'< '>` aside: those are per cursor in
/// state.rs, being what `gi` and `gv` already remember.
#[derive(Clone, Default, Debug)]
pub struct Marks {
    /// `'a`–`'z`.
    pub named: [Option<Pos>; 26],
    /// `''`: where the last jump left from (Vim's `pcmark`).
    pub context: Option<Pos>,
    /// What `context` was before the jump in progress, so a jump that went
    /// nowhere can put it back (mark.c `checkpcmark`).
    pub prev_context: Option<Pos>,
    /// `'.`: where the last change was made.
    pub change: Option<Pos>,
    /// `'[` and `']`: the first and last character of the text last changed
    /// or yanked.
    pub op_start: Option<Pos>,
    pub op_end: Option<Pos>,
    pub jumps: Jumplist,
}

impl Marks {
    /// Carry every mark through one edit.
    pub fn shift(&mut self, shift: &Shift) {
        for m in self.named.iter_mut().chain([&mut self.context, &mut self.prev_context, &mut self.change]) {
            *m = m.and_then(|p| shift.mark(p));
        }
        for m in [&mut self.op_start, &mut self.op_end] {
            *m = m.map(|p| shift.kept(p));
        }
        self.jumps.shift(shift);
    }

    /// A jump is leaving `cursor` (mark.c `setpcmark`): it becomes `''` and
    /// goes on the jumplist.
    pub fn set_context(&mut self, cursor: Pos) {
        self.prev_context = self.context;
        self.context = Some(cursor);
        self.jumps.push(cursor);
    }

    /// After a command that may have jumped: a jump that went nowhere, or
    /// whose `''` was deleted along the way (`dG`), puts the old `''` back
    /// (mark.c `checkpcmark`).
    pub fn check_context(&mut self, cursor: Pos) {
        if let Some(prev) = self.prev_context.take()
            && self.context.is_none_or(|p| p == cursor)
        {
            self.context = Some(prev);
        }
    }

    /// `count` steps back (negative, `ctrl-o`) or forward (`ctrl-i`) through
    /// the jumplist from `cursor` (mark.c `movemark`). Stepping back from
    /// the end records `cursor` first, as a jump would, so that `ctrl-i`
    /// can come back to it. `None` when the list does not reach that far.
    pub fn step_jumps(&mut self, count: isize, cursor: Pos) -> Option<Pos> {
        self.jumps.dedupe();
        let reach = |j: &Jumplist| (0..j.entries.len() as isize).contains(&(j.idx as isize + count));
        if !reach(&self.jumps) {
            return None;
        }
        if self.jumps.idx == self.jumps.entries.len() {
            self.set_context(cursor);
            self.jumps.idx -= 1;
            if !reach(&self.jumps) {
                return None;
            }
        }
        let j = &mut self.jumps;
        j.idx = (j.idx as isize + count) as usize;
        Some(j.entries[j.idx])
    }

    /// Take the lowercase marks on lines `first..=last` out, as offsets from
    /// `first`, for `:m` to put back where the lines land.
    pub fn take_lines(&mut self, first: usize, last: usize) -> Vec<(usize, Pos)> {
        let mut out = Vec::new();
        for (i, m) in self.named.iter_mut().enumerate() {
            if let Some(p) = m.filter(|p| (first..=last).contains(&p.line)) {
                out.push((i, Pos::new(p.line - first, p.col)));
                *m = None;
            }
        }
        out
    }

    /// Put back what `take_lines` took, `first` being where the lines start
    /// now.
    pub fn put_lines(&mut self, taken: Vec<(usize, Pos)>, first: usize) {
        for (i, p) in taken {
            self.named[i] = Some(Pos::new(first + p.line, p.col));
        }
    }
}

thread_local! {
    /// `'A`–`'Z`: a document and a place in it.
    static FILE_MARKS: RefCell<[Option<(String, Pos)>; 26]> = RefCell::default();
}

/// Where file mark `name` (`A`–`Z`) is.
pub fn file_mark(name: char) -> Option<(String, Pos)> {
    let i = (name as usize).checked_sub('A' as usize).filter(|&i| i < 26)?;
    FILE_MARKS.with(|m| m.borrow()[i].clone())
}

/// Set (or with `None`, delete) file mark `name`.
pub fn set_file_mark(name: char, at: Option<(String, Pos)>) {
    if let Some(i) = (name as usize).checked_sub('A' as usize).filter(|&i| i < 26) {
        FILE_MARKS.with(|m| m.borrow_mut()[i] = at);
    }
}

/// Carry the file marks set in `doc` through one of its edits; like Vim's,
/// they are moved off a deleted line rather than deleted.
pub fn shift_file_marks(doc: &str, shift: &Shift) {
    FILE_MARKS.with(|m| {
        for (d, p) in m.borrow_mut().iter_mut().flatten() {
            if d == doc {
                *p = shift.kept(*p);
            }
        }
    });
}
