//! Text objects (`:help text-objects`): `iw aw iW aW`, `is as`, `ip ap`,
//! `i" a"` and the other quotes, `i( a(` and the other brackets, `it at`.
//!
//! Each one is a port of its function in Vim's textobject.c, written over the
//! same cursor arithmetic (motion.rs `inc`/`dec`/`incl`/`decl`), because
//! what an object selects is decided by those functions' exact stopping
//! rules: which blank an `aw` takes, where `das` stops at a line end, how a
//! count or a repeated object in visual mode grows the selection. Results
//! come back in Vim's own terms — where the cursor lands, where the object
//! starts, inclusive or not, linewise or not — and state.rs applies them the
//! way Vim's operator code does.

use crate::buffer::{Buffer, Pos, chars_with_cols};
use crate::motion::{
    self, CharClass, NUL, class, dec, decl, find_unmatched, gchar, inc, incl, is_white, line_white,
    starts_paragraph,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TextObject {
    Word { big: bool },
    Sentence,
    Paragraph,
    Quote(char),
    Bracket(char, char),
    Tag,
}

impl TextObject {
    pub fn parse(ch: char) -> Option<TextObject> {
        match ch {
            'w' => Some(TextObject::Word { big: false }),
            'W' => Some(TextObject::Word { big: true }),
            's' => Some(TextObject::Sentence),
            'p' => Some(TextObject::Paragraph),
            't' => Some(TextObject::Tag),
            '"' | '\'' | '`' => Some(TextObject::Quote(ch)),
            '(' | ')' | 'b' => Some(TextObject::Bracket('(', ')')),
            '{' | '}' | 'B' => Some(TextObject::Bracket('{', '}')),
            '[' | ']' => Some(TextObject::Bracket('[', ']')),
            '<' | '>' => Some(TextObject::Bracket('<', '>')),
            _ => None,
        }
    }
}

/// The visual selection an object is typed into: its start (Vim's `VIsual`,
/// the end that stays put) and whether it is linewise. The other end is the
/// cursor. Both are inclusive char positions.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Visual {
    pub anchor: Pos,
    pub linewise: bool,
}

/// What an object selected. After an operator, `anchor` is where the text
/// starts and `cursor` where it ends — that char included when `inclusive`,
/// and only the line span counting when `linewise`. In visual mode they are
/// the new selection's ends, both inclusive; `anchor` may lie after `cursor`
/// when a selection was grown backward.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Selected {
    pub anchor: Pos,
    pub cursor: Pos,
    pub inclusive: bool,
    pub linewise: bool,
}

impl Selected {
    fn chars(anchor: Pos, cursor: Pos, inclusive: bool) -> Selected {
        Selected { anchor, cursor, inclusive, linewise: false }
    }
}

/// Apply `obj` at `cursor`, `count` times. `visual` is the selection in
/// visual mode: one that covers more than the cursor's char is grown by the
/// object rather than replaced (`vawaw`, `vipip`, `vi(i(`). None when there
/// is no such object there, which aborts the operator.
pub fn select(
    buf: &Buffer,
    cursor: Pos,
    visual: Option<Visual>,
    obj: TextObject,
    around: bool,
    count: usize,
) -> Option<Selected> {
    let ctx = Ctx { buf, cursor, visual };
    let count = count.max(1);
    match obj {
        TextObject::Word { big } => ctx.word(count, around, big),
        TextObject::Sentence => ctx.sentence(count, around),
        TextObject::Paragraph => ctx.paragraph(count, around),
        TextObject::Quote(q) => ctx.quote(count, around, q),
        TextObject::Bracket(open, close) => ctx.block(count, around, open, close),
        TextObject::Tag => ctx.tag(count, around),
    }
}

struct Ctx<'a> {
    buf: &'a Buffer,
    cursor: Pos,
    visual: Option<Visual>,
}

impl Ctx<'_> {
    /// Vim's `VIsual`, the visual selection's fixed end.
    fn vis(&self) -> Option<Pos> {
        self.visual.map(|v| v.anchor)
    }

    /// No visual selection, or one no bigger than the cursor's char: the
    /// object is selected afresh rather than extending what is there.
    fn fresh(&self) -> bool {
        self.vis().is_none_or(|v| v == self.cursor)
    }

    // ---- words -------------------------------------------------------------

    /// `iw` / `aw` (`:help aw`), textobject.c `current_word`. `iw` counts
    /// blank runs as words; `aw` takes one word plus the blanks after it — or,
    /// with none after it, the ones before it, except a line's indent.
    fn word(&self, count: usize, include: bool, big: bool) -> Option<Selected> {
        let buf = self.buf;
        let mut c = WordCursor { buf, pos: self.cursor, big };
        let mut count = count;
        let mut inclusive = true;
        let mut include_white = false;
        let mut start_pos = self.cursor;
        let mut start = self.vis().unwrap_or(self.cursor);

        if self.fresh() {
            // To the start of the word or blank run under the cursor.
            c.back_in_line();
            start_pos = c.pos;
            if (c.cls() == CharClass::Ws) == include {
                // On blanks for `aw`, or a word for `iw`: to its end.
                if !c.end_word(true, true) {
                    return None;
                }
            } else {
                // A word for `aw` (then its trailing blanks too), or blanks
                // for `iw`: to just before the next word. A one-char word at
                // a line's end backs up off the next line.
                c.fwd_word(true);
                if c.pos.col == 0 {
                    decl(buf, &mut c.pos);
                } else {
                    c.oneleft();
                }
                include_white = include;
            }
            start = start_pos;
            count -= 1;
        }

        // A count, or a selection being grown, takes more objects.
        while count > 0 {
            inclusive = true;
            if self.visual.is_some() && c.pos < start {
                // Cursor at the selection's start: grow backward.
                if decl(buf, &mut c.pos) == -1 {
                    return None;
                }
                if include != (c.cls() != CharClass::Ws) {
                    if !c.bck_word(true) {
                        return None;
                    }
                } else {
                    if !c.bckend_word(true) {
                        return None;
                    }
                    incl(buf, &mut c.pos);
                }
            } else {
                if incl(buf, &mut c.pos) == -1 {
                    return None;
                }
                if include != (c.cls() == CharClass::Ws) {
                    if !c.fwd_word(true) && count > 1 {
                        return None;
                    }
                    // Ending just past a line break must not take the next
                    // line's first char.
                    if !c.oneleft() {
                        inclusive = false;
                    }
                } else if !c.end_word(true, true) {
                    return None;
                }
            }
            count -= 1;
        }

        if include_white && (c.cls() != CharClass::Ws || (c.pos.col == 0 && !inclusive)) {
            // No blanks after the word(s): take the ones before instead,
            // which makes `daw` work on a sentence's last word — but not a
            // line's indent.
            let mut back = WordCursor { buf, pos: start_pos, big };
            if back.oneleft() {
                back.back_in_line();
                if back.cls() == CharClass::Ws && back.pos.col > 0 {
                    start = back.pos;
                }
            }
        }
        Some(Selected::chars(start, c.pos, inclusive))
    }

    // ---- sentences ---------------------------------------------------------

    /// `is` / `as` (`:help as`), textobject.c `current_sent`. `is` counts
    /// the blanks between sentences as sentences of their own; `as` takes a
    /// sentence and the blanks after it, or before it when there are none.
    fn sentence(&self, count: usize, include: bool) -> Option<Selected> {
        let buf = self.buf;
        let mut start_pos = self.cursor;
        let mut pos = start_pos;
        let mut cur = self.cursor;
        findsent(buf, &mut cur, true);

        if let Some(vis) = self.vis() {
            if start_pos != vis {
                let cur = self.sentence_extend(start_pos, pos, cur, vis, count, include);
                return Some(self.extended(vis, cur));
            }
        }

        // Starting on blanks: are they the ones just before the next
        // sentence?
        while is_white(gchar(buf, pos)) {
            incl(buf, &mut pos);
        }
        let start_blank = pos == cur;
        if start_blank {
            find_first_blank(buf, &mut start_pos);
        } else {
            findsent(buf, &mut cur, false);
            start_pos = cur;
        }
        let ncount = if include { count * 2 } else { count - usize::from(start_blank) };
        if ncount > 0 {
            findsent_forward(buf, &mut cur, ncount, true);
        } else {
            decl(buf, &mut cur);
        }

        if include {
            // Blanks taken in front mean none at the end; no blanks at the end
            // means taking the ones in front.
            if start_blank {
                find_first_blank(buf, &mut cur);
                if is_white(gchar(buf, cur)) {
                    decl(buf, &mut cur);
                }
            } else if !is_white(gchar(buf, cur)) {
                find_first_blank(buf, &mut start_pos);
            }
        }

        if let Some(vis) = self.vis() {
            // `is` on a single blank before a sentence would select nothing
            // new; grow instead, so repeating it isn't stuck.
            if start_pos == cur {
                let cur = self.sentence_extend(start_pos, pos, cur, vis, count, include);
                return Some(self.extended(vis, cur));
            }
            return Some(Selected::chars(start_pos, cur, true));
        }
        // Take the line break after the sentence, if there is one; the
        // operator pulls an end in column 0 back to the line before
        // (`:help exclusive-linewise`).
        let inclusive = incl(buf, &mut cur) == -1;
        Some(Selected::chars(start_pos, cur, inclusive))
    }

    /// Grow a visual selection by `count` sentences (twice that for `as`,
    /// which takes the blanks as well) on the cursor's side; returns the new
    /// cursor.
    fn sentence_extend(
        &self,
        start_pos: Pos,
        mut pos: Pos,
        mut cur: Pos,
        vis: Pos,
        count: usize,
        include: bool,
    ) -> Pos {
        let buf = self.buf;
        let count = if include { count * 2 } else { count };
        if start_pos < vis {
            // The cursor is the selection's start. Is it in the blanks before
            // a sentence, in a sentence, or at a sentence's start?
            let mut at_start_sent = true;
            decl(buf, &mut pos);
            while pos < cur {
                if !is_white(gchar(buf, pos)) {
                    at_start_sent = false;
                    break;
                }
                incl(buf, &mut pos);
            }
            if !at_start_sent {
                findsent(buf, &mut cur, false);
                if cur == start_pos {
                    at_start_sent = true;
                } else {
                    findsent(buf, &mut cur, true);
                }
            }
            for _ in 0..count {
                if at_start_sent {
                    find_first_blank(buf, &mut cur);
                }
                if !at_start_sent || (!include && !is_white(gchar(buf, cur))) {
                    findsent(buf, &mut cur, false);
                }
                at_start_sent = !at_start_sent;
            }
        } else {
            // The cursor is the selection's end. Is it just before a
            // sentence, in the blanks before one, or in one?
            incl(buf, &mut pos);
            let mut at_start_sent = true;
            if pos != cur {
                at_start_sent = false;
                while pos < cur {
                    if !is_white(gchar(buf, pos)) {
                        at_start_sent = true;
                        break;
                    }
                    incl(buf, &mut pos);
                }
                if at_start_sent {
                    findsent(buf, &mut cur, false);
                } else {
                    cur = start_pos;
                }
            }
            findsent_forward(buf, &mut cur, count, at_start_sent);
        }
        cur
    }

    /// The visual selection after growing it: the anchor stays and so does
    /// the mode.
    fn extended(&self, vis: Pos, cur: Pos) -> Selected {
        let linewise = self.visual.is_some_and(|v| v.linewise);
        Selected { anchor: vis, cursor: cur, inclusive: true, linewise }
    }

    // ---- paragraphs --------------------------------------------------------

    /// `ip` / `ap` (`:help ap`), textobject.c `current_par`. Always linewise.
    /// A run of blank lines is a paragraph of its own for `ip`; `ap` takes a
    /// paragraph and the blank lines after it, or before it when there are
    /// none. Unlike `{`/`}`, lines of only blanks count as blank here.
    fn paragraph(&self, count: usize, include: bool) -> Option<Selected> {
        let buf = self.buf;
        let last = buf.last_line();
        let mut start = self.cursor.line;

        if let Some(vis) = self.vis() {
            if start != vis.line {
                let line = self.paragraph_extend(start, vis.line, count, include);
                return Some(self.extended(vis, Pos::new(line, 0)));
            }
        }

        // Back to the first line of the paragraph or blank run.
        let white_in_front = line_white(buf, start);
        while start > 0 {
            if white_in_front {
                if !line_white(buf, start - 1) {
                    break;
                }
            } else if line_white(buf, start - 1) || starts_paragraph(buf, start) {
                break;
            }
            start -= 1;
        }

        // Past any blank lines; `end` then sits on the line before the next
        // paragraph (it is signed: that may be the line before line 0).
        let mut end = start as isize;
        while end <= last as isize && line_white(buf, end as usize) {
            end += 1;
        }
        end -= 1;
        let white = |l: isize| line_white(buf, l as usize);
        let mut i = count;
        if !include && white_in_front {
            i -= 1;
        }
        while i > 0 {
            i -= 1;
            if end == last as isize {
                return None;
            }
            let do_white = !include && white(end + 1);
            if include || !do_white {
                // To the paragraph's end.
                end += 1;
                while end < last as isize
                    && !white(end + 1)
                    && !starts_paragraph(buf, (end + 1) as usize)
                {
                    end += 1;
                }
            }
            if i == 0 && white_in_front && include {
                break;
            }
            // To the end of the blank lines after it.
            if include || do_white {
                while end < last as isize && white(end + 1) {
                    end += 1;
                }
            }
        }
        let end = end as usize;

        // No blank lines at the end: take the ones at the start instead.
        if !white_in_front && !line_white(buf, end) && include {
            while start > 0 && line_white(buf, start - 1) {
                start -= 1;
            }
        }

        if let Some(v) = self.visual {
            // `Vipipip` on a lone blank line would select the same line over
            // and over; grow from it instead.
            if v.linewise && start == self.cursor.line {
                let line = self.paragraph_extend(start, v.anchor.line, count, include);
                return Some(self.extended(v.anchor, Pos::new(line, 0)));
            }
            let anchor = if v.anchor.line != start { Pos::new(start, 0) } else { v.anchor };
            let cursor = Pos::new(end, 0);
            return Some(Selected { anchor, cursor, inclusive: true, linewise: true });
        }
        Some(Selected {
            anchor: Pos::new(start, 0),
            cursor: Pos::new(end, 0),
            inclusive: true,
            linewise: true,
        })
    }

    /// Grow a visual selection spanning lines by `count` paragraphs on the
    /// cursor's side (`ap` takes the blank lines that follow each, too);
    /// returns the cursor's new line. Stops at the buffer's edge.
    fn paragraph_extend(&self, from: usize, vis_line: usize, count: usize, include: bool) -> usize {
        let buf = self.buf;
        let back = from < vis_line;
        let edge = if back { 0 } else { buf.last_line() };
        let step = |l: usize| if back { l - 1 } else { l + 1 };
        let mut line = from;
        for _ in 0..count {
            if line == edge {
                break;
            }
            let mut prev_white = None;
            for _ in 0..2 {
                line = step(line);
                let white = line_white(buf, line);
                if prev_white == Some(white) {
                    line = if back { line + 1 } else { line - 1 };
                    break;
                }
                while line != edge {
                    let next = step(line);
                    let boundary = if back { line } else { next };
                    if white != line_white(buf, next)
                        || (!white && starts_paragraph(buf, boundary))
                    {
                        break;
                    }
                    line = next;
                }
                if !include || line == edge {
                    break;
                }
                prev_white = Some(white);
            }
        }
        line
    }

    // ---- quotes ------------------------------------------------------------

    /// `i"` / `a"` and the other quotes (`:help a"`), textobject.c
    /// `current_quote`. One line only. A cursor on a quote pairs the line's
    /// quotes up from its start to see which string it belongs to; elsewhere
    /// the nearest quote before the cursor opens the string (or, with none
    /// before it, the first one after). `\` escapes a quote (`'quoteescape'`).
    /// `a"` takes the blanks after the closing quote, or with none the ones
    /// before the opening one. `2i"` takes the quotes but no blanks; in visual
    /// mode a repeated `i"` takes the quotes and `a"` the next string.
    fn quote(&self, count: usize, include: bool, q: char) -> Option<Selected> {
        let lnum = self.cursor.line;
        let cols = chars_with_cols(self.buf.line(lnum));
        let line_len = self.buf.line_len(lnum);
        let at = |i: usize| cols.get(i).map_or(NUL, |&(_, c)| c);
        let index = |col: usize| cols.partition_point(|&(c, _)| c < col);
        let pos = |i: usize| Pos::new(lnum, cols.get(i).map_or(line_len, |&(c, _)| c));
        let next_quote = |from: usize, escape: bool| find_next_quote(&cols, from, q, escape);
        let prev_quote = |from: usize, escape: bool| find_prev_quote(&cols, from, q, escape);

        let cursor = index(self.cursor.col);
        let mut vis_empty = true;
        let mut vis_bef_curs = false;
        let mut vis = cursor;
        if let Some(v) = self.vis() {
            if v.line != lnum {
                return None;
            }
            vis = index(v.col);
            vis_bef_curs = vis < cursor;
            vis_empty = vis == cursor;
        }

        // Does the selection span exactly a string's inside, and does it hold
        // a quote?
        let mut inside_quotes = false;
        let mut selected_quote = false;
        if !vis_empty {
            let (lo, hi) = if vis_bef_curs { (vis, cursor) } else { (cursor, vis) };
            inside_quotes = lo > 0 && at(lo - 1) == q && at(hi) != NUL && at(hi + 1) == q;
            selected_quote = (lo..=hi).take_while(|&i| at(i) != NUL).any(|i| at(i) == q);
        }

        let mut col_start = cursor;
        let mut col_end;
        if !vis_empty && at(col_start) == q {
            // Selecting already and on a quote: the next string over.
            if vis_bef_curs {
                col_start = next_quote(col_start + 1, false)?;
                match next_quote(col_start + 1, true) {
                    Some(e) => col_end = e,
                    None => {
                        col_end = col_start;
                        col_start = cursor;
                    }
                }
            } else {
                col_end = prev_quote(col_start, false);
                if at(col_end) != q {
                    return None;
                }
                col_start = prev_quote(col_end, true);
                if at(col_start) != q {
                    col_start = col_end;
                    col_end = cursor;
                }
            }
        } else if at(col_start) == q || !vis_empty {
            // On a quote, opening or closing: pair them from the line's start.
            let first_col = if vis_empty {
                Some(col_start)
            } else if vis_bef_curs {
                next_quote(col_start, false)
            } else {
                Some(prev_quote(col_start, false))
            };
            let first_col = first_col?;
            col_start = 0;
            loop {
                col_start = next_quote(col_start, false)?;
                if col_start > first_col {
                    return None;
                }
                col_end = next_quote(col_start + 1, true)?;
                if col_start <= first_col && first_col <= col_end {
                    break;
                }
                col_start = col_end + 1;
            }
        } else {
            // Back to the opening quote, or ahead to one when there is none
            // before the cursor.
            col_start = prev_quote(col_start, true);
            if at(col_start) != q {
                col_start = next_quote(col_start, false)?;
            }
            col_end = next_quote(col_start + 1, true)?;
        }

        if include {
            if is_white(at(col_end + 1)) {
                while is_white(at(col_end + 1)) {
                    col_end += 1;
                }
            } else {
                while col_start > 0 && is_white(at(col_start - 1)) {
                    col_start -= 1;
                }
            }
        }
        // After `vi"` another `i"` takes the quotes; so does `2i"`.
        if !include && count < 2 && (vis_empty || !inside_quotes) {
            col_start += 1;
        }

        let mut end = col_end;
        let mut inclusive = false;
        if include || count > 1 || (!vis_empty && inside_quotes) {
            end += 1;
            inclusive = end >= cols.len();
        }
        if self.visual.is_none() {
            return Some(Selected::chars(pos(col_start), pos(end), inclusive));
        }
        let mut anchor = vis;
        let mut cur = end;
        if vis_empty
            || (vis_bef_curs
                && !selected_quote
                && (inside_quotes || (at(vis) != q && (vis == 0 || at(vis - 1) != q))))
        {
            anchor = col_start;
        }
        if vis_empty || vis_bef_curs {
            cur = cur.saturating_sub(1);
        } else {
            // The cursor is the selection's start: the end moves only when it
            // sat just inside the quotes or not at a quote.
            if inside_quotes
                || (!selected_quote && at(vis) != q && (at(vis) == NUL || at(vis + 1) != q))
            {
                anchor = cur.saturating_sub(1);
            }
            cur = col_start;
        }
        Some(Selected::chars(pos(anchor), pos(cur), true))
    }

    // ---- brackets ----------------------------------------------------------

    /// `i(` / `a(` and the other brackets (`:help a(`), textobject.c
    /// `current_block`: out to the `count`-th unclosed `open` before the
    /// cursor, or with the cursor in no block, ahead to the next `open`.
    /// A cursor on either bracket means that pair. `i{` leaves out the brace
    /// lines when they hold nothing else, so the operator turns linewise
    /// (`:help exclusive-linewise`); an empty `()` is an empty range, which
    /// `ci(` can still insert into. In visual mode a repeat grows outward.
    fn block(&self, count: usize, include: bool, what: char, other: char) -> Option<Selected> {
        let buf = self.buf;
        let visual = self.visual.is_some();
        let mut cur = self.cursor;
        let mut old_start = cur;
        let mut old_end = cur;
        // Vim forces `cpo-%` only while it looks for the opening bracket, so
        // one inside a string counts there; the closing one is looked for
        // with `%`'s own smart quotes. (Hence `di(` fails from the `x` in
        // `s("(", x)`: the `(` found is in the string, and the scan out of it
        // takes the rest of the line for a string.)
        let open_before =
            |from: Pos, smart: bool| find_unmatched(buf, from, other, what, false, smart, false);
        let close_after = |from: Pos| find_unmatched(buf, from, what, other, true, true, false);

        if self.fresh() {
            if what == '{' {
                // Ignore indent.
                while in_indent(buf, cur, 1) {
                    if inc(buf, &mut cur) != 0 {
                        break;
                    }
                }
            }
            if gchar(buf, cur) == what {
                // On the opening bracket: start just after it.
                cur.col += what.len_utf16();
            }
        } else if let Some(vis) = self.vis() {
            if vis < cur {
                old_start = vis;
                cur = vis;
            } else {
                old_end = vis;
            }
        }

        let mut start_pos = cur;
        if open_before(cur, false).is_some() {
            for _ in 0..count {
                start_pos = open_before(cur, false)?;
                cur = start_pos;
            }
        } else {
            for _ in 0..count {
                start_pos = find_unmatched(buf, cur, other, what, true, false, false)?;
                cur = start_pos;
            }
        }
        let mut end_pos = close_after(cur)?;
        cur = end_pos;

        // `i(`: leave the brackets out, and an end bracket that only indent
        // precedes leaves that line out too — unless that makes the area no
        // bigger than what visual mode started with; then go out a level.
        let mut sol = false;
        if !include {
            loop {
                incl(buf, &mut start_pos);
                sol = cur.col == 0;
                decl(buf, &mut cur);
                while in_indent(buf, cur, 1) {
                    sol = true;
                    if decl(buf, &mut cur) != 0 {
                        break;
                    }
                }
                if visual && start_pos == end_pos {
                    // Nothing inside to select.
                    return None;
                }
                if visual && start_pos >= old_start && old_end >= cur && start_pos != cur {
                    cur = old_start;
                    decl(buf, &mut cur);
                    start_pos = open_before(cur, true)?;
                    end_pos = close_after(start_pos)?;
                    cur = end_pos;
                } else {
                    break;
                }
            }
        }

        if visual {
            if sol && gchar(buf, cur) != NUL {
                // Include the line break (state.rs clamps this back to the
                // last char; see the README).
                inc(buf, &mut cur);
            }
            return Some(Selected::chars(start_pos, cur, true));
        }
        let mut inclusive = false;
        if sol {
            incl(buf, &mut cur);
        } else if start_pos <= cur {
            inclusive = true;
        } else {
            // Nothing between the brackets: an empty range.
            cur = start_pos;
        }
        Some(Selected::chars(start_pos, cur, inclusive))
    }

    // ---- tags --------------------------------------------------------------

    /// `it` / `at` (`:help tag-blocks`), textobject.c `current_tagblock`:
    /// out to the `count`-th unclosed `<aaa>` before the cursor and on to its
    /// `</aaa>`, ignoring case. `<aaa/>` is skipped, a start tag with no end
    /// (`<br>`) is passed over for the one enclosing it, and stray end tags
    /// are ignored. A cursor on a tag means that tag's block. `it` on a block
    /// already selected takes the tags too.
    fn tag(&self, count_arg: usize, include: bool) -> Option<Selected> {
        let buf = self.buf;
        let visual = self.visual.is_some();
        let mut cur = self.cursor;
        let mut old_start = cur;
        let mut old_end = cur;
        if !visual {
            decl(buf, &mut old_end); // inclusive
        }

        if self.fresh() {
            // Ignore indent.
            while in_indent(buf, cur, 1) {
                if inc(buf, &mut cur) != 0 {
                    break;
                }
            }
            if in_html_tag(buf, cur, false) {
                // On a start tag: to its `>`, so it is the one found.
                while gchar(buf, cur) != '>' {
                    if inc(buf, &mut cur) < 0 {
                        break;
                    }
                }
            } else if in_html_tag(buf, cur, true) {
                // On an end tag: to just before it.
                while gchar(buf, cur) != '<' {
                    if dec(buf, &mut cur) < 0 {
                        break;
                    }
                }
                dec(buf, &mut cur);
                old_end = cur;
            }
        } else if let Some(vis) = self.vis() {
            if vis < cur {
                old_start = vis;
                cur = vis;
            } else {
                old_end = vis;
            }
        }

        let mut do_include = include;
        let mut count = count_arg;
        let mut is_inclusive = true;
        let (start_pos, end_pos) = loop {
            for _ in 0..count {
                cur = tag_pair(buf, cur, false, None)?;
            }
            let mut start_pos = cur;

            // Find the matching end tag by the start tag's name.
            inc(buf, &mut cur);
            let text = buf.line(cur.line);
            let name: Vec<char> = text[crate::buffer::utf16_to_byte(text, cur.col)..]
                .chars()
                .take_while(|&c| c != '>' && !is_white(c))
                .collect();
            if name.is_empty() {
                return None;
            }
            match tag_pair(buf, cur, true, Some(&name)) {
                Some(end) if end >= old_end => cur = end,
                _ => {
                    // No end tag, or one before where we started: an HTML tag
                    // without one, like `<br>`. Try the enclosing start tag.
                    count = 1;
                    cur = start_pos;
                    continue;
                }
            }

            if do_include {
                // Up to the end tag's `>`.
                while gchar(buf, cur) != '>' {
                    if inc(buf, &mut cur) < 0 {
                        break;
                    }
                }
            } else if gchar(buf, cur) == '<' {
                // Leave the end tag out. One opening its line stays put and
                // the range turns exclusive, taking the line break before it.
                if !visual && cur.col == 0 {
                    is_inclusive = false;
                } else {
                    dec(buf, &mut cur);
                }
            }
            let end_pos = cur;

            if !do_include {
                // Leave the start tag out, minding a `>` inside quotes.
                let mut in_quotes = false;
                cur = start_pos;
                while inc(buf, &mut cur) >= 0 {
                    let c = gchar(buf, cur);
                    if c == '>' && !in_quotes {
                        inc(buf, &mut cur);
                        start_pos = cur;
                        break;
                    } else if c == '"' || c == '\'' {
                        in_quotes = !in_quotes;
                    }
                }
                if visual && start_pos == old_start && end_pos == old_end {
                    // The same text as selected already: take the tags too.
                    do_include = true;
                    cur = old_start;
                    count = count_arg;
                    continue;
                }
            }
            break (start_pos, end_pos);
        };

        if end_pos < start_pos {
            // Nothing between the tags: an empty range (in visual mode, the
            // char after the start tag).
            let inclusive = visual;
            return Some(Selected::chars(start_pos, start_pos, inclusive));
        }
        let inclusive = visual || is_inclusive;
        Some(Selected::chars(start_pos, end_pos, inclusive))
    }
}

// ---- sentence helpers ------------------------------------------------------

/// Vim's `findsent(dir, 1)` on a cursor, which stays put when it fails.
fn findsent(buf: &Buffer, cur: &mut Pos, forward: bool) {
    if let Some(p) = motion::sentence(buf, *cur, forward, 1) {
        *cur = p;
    }
}

/// Back to the first of the blanks before `p`.
fn find_first_blank(buf: &Buffer, p: &mut Pos) {
    while decl(buf, p) != -1 {
        if !is_white(gchar(buf, *p)) {
            incl(buf, p);
            break;
        }
    }
}

/// Over `count` halves of sentences — alternately a sentence and the blanks
/// after it — leaving the cursor on the last char taken.
fn findsent_forward(buf: &Buffer, cur: &mut Pos, count: usize, mut at_start_sent: bool) {
    for left in (0..count).rev() {
        findsent(buf, cur, true);
        if at_start_sent {
            find_first_blank(buf, cur);
        }
        if left == 0 || at_start_sent {
            decl(buf, cur);
        }
        at_start_sent = !at_start_sent;
    }
}

/// Vim's `inindent(extra)`: whether the line's indent reaches `extra` chars
/// past `p` — with 1, whether `p` sits inside the indent.
fn in_indent(buf: &Buffer, p: Pos, extra: usize) -> bool {
    let indent = buf.line(p.line).chars().take_while(|&c| is_white(c)).count();
    indent >= p.col + extra
}

// ---- word helpers ----------------------------------------------------------

/// A cursor moving the way textobject.c's word functions move Vim's: their
/// stopping rules (an empty line is a word, `eol` stops at a line's end, a
/// line's end is a blank) are what `iw`/`aw` select by. Each moves one word.
struct WordCursor<'a> {
    buf: &'a Buffer,
    pos: Pos,
    big: bool,
}

impl WordCursor<'_> {
    /// Vim's `cls()`: a line's end counts as a blank.
    fn cls(&self) -> CharClass {
        match gchar(self.buf, self.pos) {
            ' ' | '\t' | NUL => CharClass::Ws,
            c => class(c, self.big),
        }
    }

    fn inc(&mut self) -> i32 {
        inc(self.buf, &mut self.pos)
    }

    fn dec(&mut self) -> i32 {
        dec(self.buf, &mut self.pos)
    }

    /// Vim's `oneleft()`: one char left, never off the line.
    fn oneleft(&mut self) -> bool {
        if self.pos.col == 0 {
            return false;
        }
        self.dec();
        true
    }

    fn on_empty_line(&self) -> bool {
        self.pos.col == 0 && self.buf.line_len(self.pos.line) == 0
    }

    /// Back to the start of the word or blank run, within the line.
    fn back_in_line(&mut self) {
        let sclass = self.cls();
        while self.pos.col != 0 {
            self.dec();
            if self.cls() != sclass {
                self.inc();
                break;
            }
        }
    }

    /// Vim's `skip_chars`: over a run of `cclass`; true at the buffer's edge.
    fn skip_chars(&mut self, cclass: CharClass, forward: bool) -> bool {
        while self.cls() == cclass {
            let r = if forward { self.inc() } else { self.dec() };
            if r == -1 {
                return true;
            }
        }
        false
    }

    /// Vim's `fwd_word`: to the next word's start; with `eol`, no further
    /// than the line's end. False when it started on the buffer's last char.
    fn fwd_word(&mut self, eol: bool) -> bool {
        let sclass = self.cls();
        let last_line = self.pos.line == self.buf.last_line();
        let i = self.inc();
        if i == -1 || (i >= 1 && last_line) {
            return false;
        }
        if i >= 1 && eol {
            return true;
        }
        if sclass != CharClass::Ws {
            while self.cls() == sclass {
                let i = self.inc();
                if i == -1 || (i >= 1 && eol) {
                    return true;
                }
            }
        }
        while self.cls() == CharClass::Ws {
            if self.on_empty_line() {
                break;
            }
            let i = self.inc();
            if i == -1 || (i >= 1 && eol) {
                return true;
            }
        }
        true
    }

    /// Vim's `bck_word`: to the previous word's start; with `stop`, a cursor
    /// already on a word's start stays. False at the buffer's start.
    fn bck_word(&mut self, stop: bool) -> bool {
        let sclass = self.cls();
        if self.dec() == -1 {
            return false;
        }
        if !stop || sclass == self.cls() || sclass == CharClass::Ws {
            while self.cls() == CharClass::Ws {
                if self.on_empty_line() || self.dec() == -1 {
                    return true;
                }
            }
            if self.skip_chars(self.cls(), false) {
                return true;
            }
        }
        self.inc(); // overshot
        true
    }

    /// Vim's `end_word`: to the word's end, or the next word's; with `stop`, a
    /// cursor already on a word's end stays, and with `empty` an empty line
    /// stops it. False at the buffer's end.
    fn end_word(&mut self, stop: bool, empty: bool) -> bool {
        let sclass = self.cls();
        if self.inc() == -1 {
            return false;
        }
        if self.cls() == sclass && sclass != CharClass::Ws {
            if self.skip_chars(sclass, true) {
                return false;
            }
        } else if !stop || sclass == CharClass::Ws {
            while self.cls() == CharClass::Ws {
                if empty && self.on_empty_line() {
                    return true;
                }
                if self.inc() == -1 {
                    return false;
                }
            }
            if self.skip_chars(self.cls(), true) {
                return false;
            }
        }
        self.dec(); // overshot
        true
    }

    /// Vim's `bckend_word`: to the previous word's end; with `eol`, no
    /// further than the line's start. False at the buffer's start.
    fn bckend_word(&mut self, eol: bool) -> bool {
        let sclass = self.cls();
        let i = self.dec();
        if i == -1 {
            return false;
        }
        if eol && i == 1 {
            return true;
        }
        if sclass != CharClass::Ws {
            while self.cls() == sclass {
                let i = self.dec();
                if i == -1 || (eol && i == 1) {
                    return true;
                }
            }
        }
        while self.cls() == CharClass::Ws {
            if self.on_empty_line() {
                break;
            }
            let i = self.dec();
            if i == -1 || (eol && i == 1) {
                return true;
            }
        }
        true
    }
}

// ---- quote helpers ---------------------------------------------------------

/// Vim's `find_next_quote`: the char index of the first `q` at or after
/// `from`, stepping over `\`-escaped chars when `escape` is set.
fn find_next_quote(cols: &[(usize, char)], from: usize, q: char, escape: bool) -> Option<usize> {
    let mut i = from;
    loop {
        let c = cols.get(i)?.1;
        if escape && c == '\\' {
            i += 1;
            cols.get(i)?;
        } else if c == q {
            return Some(i);
        }
        i += 1;
    }
}

/// Vim's `find_prev_quote`: the char index of the last `q` before `from`,
/// ignoring ones escaped by an odd run of `\` when `escape` is set; 0 when
/// there is none (callers check the char there).
fn find_prev_quote(cols: &[(usize, char)], from: usize, q: char, escape: bool) -> usize {
    let at = |i: usize| cols.get(i).map_or(NUL, |&(_, c)| c);
    let mut i = from;
    while i > 0 {
        i -= 1;
        let mut n = 0;
        if escape {
            while i - n > 0 && at(i - n - 1) == '\\' {
                n += 1;
            }
        }
        if n % 2 == 1 {
            i -= n;
        } else if at(i) == q {
            break;
        }
    }
    i
}

// ---- tag helpers -----------------------------------------------------------

/// Vim's `in_html_tag`: whether `p` is on a start tag (`end_tag` false; not a
/// self-closing `<aaa/>`) or an end tag (`end_tag` true), judged by the
/// nearest `<` at or before it with no `>` in between.
fn in_html_tag(buf: &Buffer, p: Pos, end_tag: bool) -> bool {
    let cols = chars_with_cols(buf.line(p.line));
    let at = |i: usize| cols.get(i).map_or(NUL, |&(_, c)| c);
    let mut k = cols.partition_point(|&(c, _)| c < p.col);
    while k > 0 {
        if at(k) == '<' {
            break;
        }
        k -= 1;
        if at(k) == '>' {
            break;
        }
    }
    if at(k) != '<' {
        return false;
    }
    if end_tag {
        return at(k + 1) == '/';
    }
    if at(k + 1) == '/' {
        return false;
    }
    // The tag's `>` must not follow a `/`.
    let mut q = Pos::new(p.line, cols[k].0);
    let mut last = NUL;
    loop {
        if inc(buf, &mut q) < 0 {
            return false;
        }
        let c = gchar(buf, q);
        if c == '>' {
            return last != '/';
        }
        last = c;
    }
}

/// Vim's `searchpair()` for tags, as current_tagblock runs it: backward,
/// the first start tag no end tag between closes; forward, the first end
/// tag no start tag between opens. Without a `name` any tag counts (Vim's
/// `<[^ \t>/!]\+\%(…\)` and `</[^>]*>`); with one, only tags of that name,
/// ignoring case. Like any search, a match at `from` itself doesn't count.
fn tag_pair(buf: &Buffer, from: Pos, forward: bool, name: Option<&[char]>) -> Option<Pos> {
    let mut nest = 1usize;
    let mut line = from.line;
    loop {
        let cols = chars_with_cols(buf.line(line));
        let candidates: Box<dyn Iterator<Item = &(usize, char)>> = if forward {
            Box::new(cols.iter().filter(|&&(c, _)| line != from.line || c > from.col))
        } else {
            Box::new(cols.iter().rev().filter(|&&(c, _)| line != from.line || c < from.col))
        };
        for &(col, ch) in candidates {
            if ch != '<' {
                continue;
            }
            let p = Pos::new(line, col);
            let opens = if start_tag(buf, p, name) {
                forward
            } else if end_tag(buf, p, name) {
                !forward
            } else {
                continue;
            };
            if opens {
                nest += 1;
            } else {
                nest -= 1;
                if nest == 0 {
                    return Some(p);
                }
            }
        }
        if forward {
            if line == buf.last_line() {
                return None;
            }
            line += 1;
        } else {
            line = line.checked_sub(1)?;
        }
    }
}

/// Whether a start tag begins at the `<` at `p`. Any name (Vim's
/// `<[^ \t>/!]\+\%(\_s\_[^>]\{-}[^/]>\|$\|\_s\=>\)`), or the one given
/// (`<name\>\%(\_s\_[^>]\{-}\_[^/]>\|\_s\?>\)\c`): the name, then `>` or a
/// blank and attributes up to a `>` that no `/` precedes. A self-closing
/// `<aaa/>` is no start tag.
fn start_tag(buf: &Buffer, p: Pos, name: Option<&[char]>) -> bool {
    let cols = chars_with_cols(buf.line(p.line));
    let i = cols.partition_point(|&(c, _)| c < p.col);
    let mut j = i + 1;
    match name {
        None => {
            while j < cols.len() && !matches!(cols[j].1, ' ' | '\t' | '>' | '/' | '!') {
                j += 1;
            }
            if j == i + 1 {
                return false;
            }
            if j == cols.len() {
                return true; // the `$` branch: attributes follow on later lines
            }
        }
        Some(name) => {
            for &n in name {
                if j >= cols.len() || !same_ignoring_case(cols[j].1, n) {
                    return false;
                }
                j += 1;
            }
            // `\>`: the name has to end a word.
            if !name.last().is_some_and(|&c| is_word_char(c)) {
                return false;
            }
        }
    }
    // Past the one blank (a line break counts) that opens the attributes.
    let attrs = if j == cols.len() {
        if p.line == buf.last_line() {
            return false;
        }
        Pos::new(p.line + 1, 0)
    } else {
        match cols[j].1 {
            '>' => return true,
            ' ' | '\t' => Pos::new(p.line, cols[j].0 + 1),
            _ => return false,
        }
    };
    let Some(gt) = find_forward(buf, attrs, '>') else {
        return false;
    };
    if gt == attrs {
        return true;
    }
    if gt.col == 0 {
        // The char before the `>` is a line break, which only the named
        // pattern's `\_[^/]` accepts.
        return name.is_some();
    }
    let mut before = gt;
    dec(buf, &mut before);
    gchar(buf, before) != '/'
}

/// Whether an end tag begins at the `<` at `p`: `</` and a `>` later on the
/// line, or `</name>` exactly (ignoring case) when a name is given.
fn end_tag(buf: &Buffer, p: Pos, name: Option<&[char]>) -> bool {
    let cols = chars_with_cols(buf.line(p.line));
    let i = cols.partition_point(|&(c, _)| c < p.col);
    if cols.get(i + 1).map(|&(_, c)| c) != Some('/') {
        return false;
    }
    let rest = &cols[i + 2..];
    match name {
        None => rest.iter().any(|&(_, c)| c == '>'),
        Some(name) => {
            rest.len() > name.len()
                && name.iter().zip(rest).all(|(&n, &(_, c))| same_ignoring_case(c, n))
                && rest[name.len()].1 == '>'
        }
    }
}

/// The first `ch` at or after `from`, across lines.
fn find_forward(buf: &Buffer, from: Pos, ch: char) -> Option<Pos> {
    for line in from.line..buf.line_count() {
        let found = chars_with_cols(buf.line(line))
            .into_iter()
            .find(|&(c, x)| x == ch && (line != from.line || c >= from.col));
        if let Some((col, _)) = found {
            return Some(Pos::new(line, col));
        }
    }
    None
}

fn same_ignoring_case(a: char, b: char) -> bool {
    a == b || a.to_lowercase().eq(b.to_lowercase())
}

/// Vim's default `'iskeyword'`: letters, digits and `_`.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn buf(s: &str) -> Buffer {
        Buffer::from_text(s)
    }

    /// An object after an operator, as the `[start, end)` it operates on
    /// (before the operator's exclusive-linewise adjustment).
    fn range(b: &Buffer, pos: Pos, obj: TextObject, around: bool) -> Option<(Pos, Pos)> {
        let sel = select(b, pos, None, obj, around, 1)?;
        let end = if sel.inclusive { motion::right(b, sel.cursor, 1) } else { sel.cursor };
        Some((sel.anchor, end))
    }

    #[test]
    fn inner_and_around_word() {
        let b = buf("foo bar baz");
        let (s, e) = range(&b, Pos::new(0, 5), TextObject::Word { big: false }, false).unwrap();
        assert_eq!((s, e), (Pos::new(0, 4), Pos::new(0, 7)));
        let (s, e) = range(&b, Pos::new(0, 5), TextObject::Word { big: false }, true).unwrap();
        assert_eq!((s, e), (Pos::new(0, 4), Pos::new(0, 8)));
    }

    #[test]
    fn around_word_leading_ws_fallback() {
        let b = buf("foo bar");
        let (s, e) = range(&b, Pos::new(0, 5), TextObject::Word { big: false }, true).unwrap();
        assert_eq!((s, e), (Pos::new(0, 3), Pos::new(0, 7)));
    }

    #[test]
    fn inner_quote() {
        let b = buf(r#"say "hi there" now"#);
        let (s, e) = range(&b, Pos::new(0, 7), TextObject::Quote('"'), false).unwrap();
        assert_eq!((s, e), (Pos::new(0, 5), Pos::new(0, 13)));
        // Cursor before the pair uses the next pair on the line.
        let (s, e) = range(&b, Pos::new(0, 0), TextObject::Quote('"'), false).unwrap();
        assert_eq!((s, e), (Pos::new(0, 5), Pos::new(0, 13)));
    }

    #[test]
    fn around_quote_takes_trailing_space() {
        let b = buf(r#"a "b" c"#);
        let (s, e) = range(&b, Pos::new(0, 3), TextObject::Quote('"'), true).unwrap();
        assert_eq!((s, e), (Pos::new(0, 2), Pos::new(0, 6)));
    }

    #[test]
    fn inner_brackets_nested_multiline() {
        let b = buf("f(a, g(b),\n  c)");
        let (s, e) = range(&b, Pos::new(0, 3), TextObject::Bracket('(', ')'), false).unwrap();
        assert_eq!((s, e), (Pos::new(0, 2), Pos::new(1, 3)));
        let (s, e) = range(&b, Pos::new(0, 7), TextObject::Bracket('(', ')'), false).unwrap();
        assert_eq!((s, e), (Pos::new(0, 7), Pos::new(0, 8)));
        let (s, e) = range(&b, Pos::new(0, 3), TextObject::Bracket('(', ')'), true).unwrap();
        assert_eq!((s, e), (Pos::new(0, 1), Pos::new(1, 4)));
    }

    #[test]
    fn bracket_on_the_brackets_themselves() {
        let b = buf("(abc)");
        let (s, e) = range(&b, Pos::new(0, 0), TextObject::Bracket('(', ')'), false).unwrap();
        assert_eq!((s, e), (Pos::new(0, 1), Pos::new(0, 4)));
        let (s, e) = range(&b, Pos::new(0, 4), TextObject::Bracket('(', ')'), false).unwrap();
        assert_eq!((s, e), (Pos::new(0, 1), Pos::new(0, 4)));
    }

    #[test]
    fn no_object_outside() {
        let b = buf("plain text");
        assert_eq!(range(&b, Pos::new(0, 0), TextObject::Bracket('(', ')'), false), None);
        assert_eq!(range(&b, Pos::new(0, 0), TextObject::Quote('"'), false), None);
        assert_eq!(range(&b, Pos::new(0, 0), TextObject::Tag, false), None);
    }

    #[test]
    fn start_and_end_tags() {
        let b = buf("<a href=\"x\"> <br/> <img src=x /> </A> <div\n  id=1> <p>");
        let start = |col| start_tag(&b, Pos::new(0, col), None);
        assert!(start(0));
        assert!(!start(13)); // <br/>
        assert!(!start(19)); // <img … />
        assert!(!start(33)); // </A>
        assert!(start(38)); // <div at the line's end
        assert!(end_tag(&b, Pos::new(0, 33), Some(&['a'])));
        assert!(!end_tag(&b, Pos::new(0, 33), Some(&['b'])));
        let named = buf("<div\n  id=1>");
        assert!(start_tag(&named, Pos::new(0, 0), Some(&['D', 'I', 'V'])));
        assert!(!start_tag(&named, Pos::new(0, 0), Some(&['d', 'i'])));
    }
}
