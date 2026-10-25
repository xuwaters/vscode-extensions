//! End-to-end tests: feed keys, assert buffer text, cursor, and effects.
//! The engine self-applies its edits, so the buffer reflects each keystroke
//! immediately; insert-mode typing is simulated the way the host delivers it
//! (document change + cursor move).

use pretty_assertions::assert_eq;
use vim_engine::buffer::{Buffer, Pos};
use vim_engine::state::{Command, EasyUi, Edit, Effects, SearchUi, Session};
use vim_engine::Key;

fn session(text: &str) -> Session {
    Session::new(text)
}

fn at(text: &str, line: usize, col: usize) -> Session {
    let mut s = Session::new(text);
    s.reset(text, line, col);
    s
}

/// Feed a key sequence; specials in angle brackets: "dw<esc>x". Returns the
/// last key's effects.
fn feed(s: &mut Session, keys: &str) -> Effects {
    let mut last = None;
    let mut chars = keys.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '<' {
            let mut name = String::from("<");
            for c in chars.by_ref() {
                name.push(c);
                if c == '>' {
                    break;
                }
            }
            last = Some(s.key(Key::parse(&name).expect("special key")));
        } else {
            last = Some(s.key(Key::Char(ch)));
        }
    }
    last.expect("at least one key")
}

/// Feed every char as itself, with no `<key>` parsing — for patterns that
/// contain angle brackets.
fn feed_literal(s: &mut Session, keys: &str) {
    for ch in keys.chars() {
        s.key(Key::Char(ch));
    }
}

/// Simulate insert-mode typing as the host delivers it.
fn type_ins(s: &mut Session, text: &str) {
    assert_eq!(s.mode_label(), "insert", "typing requires insert mode");
    let pos = s.cursor();
    s.apply_change(pos, pos, text);
    let lines: Vec<&str> = text.split('\n').collect();
    let (line, col) = if lines.len() == 1 {
        (pos.line, pos.col + text.encode_utf16().count())
    } else {
        (
            pos.line + lines.len() - 1,
            lines.last().unwrap().encode_utf16().count(),
        )
    };
    s.set_position(line, col);
}

#[test]
fn basic_hjkl_and_clamping() {
    let mut s = session("abc\nde\nfghij");
    feed(&mut s, "ll");
    assert_eq!(s.cursor(), Pos::new(0, 2));
    feed(&mut s, "l"); // clamped at line end
    assert_eq!(s.cursor(), Pos::new(0, 2));
    feed(&mut s, "j"); // desired col 2 > "de" -> clamp to 1
    assert_eq!(s.cursor(), Pos::new(1, 1));
    feed(&mut s, "j"); // sticky column returns
    assert_eq!(s.cursor(), Pos::new(2, 2));
    feed(&mut s, "kkhh");
    assert_eq!(s.cursor(), Pos::new(0, 0));
}

#[test]
fn dollar_stays_sticky() {
    let mut s = session("long line\nab\nlonger line");
    feed(&mut s, "$");
    assert_eq!(s.cursor(), Pos::new(0, 8));
    feed(&mut s, "j");
    assert_eq!(s.cursor(), Pos::new(1, 1));
    feed(&mut s, "j");
    assert_eq!(s.cursor(), Pos::new(2, 10));
}

#[test]
fn ctrl_n_and_ctrl_p_are_j_and_k() {
    let mut s = session("abc\nde\nfghij");
    feed(&mut s, "ll<c-n>");
    assert_eq!(s.cursor(), Pos::new(1, 1));
    feed(&mut s, "<c-n>"); // the sticky column, as j
    assert_eq!(s.cursor(), Pos::new(2, 2));
    feed(&mut s, "2<c-p>");
    assert_eq!(s.cursor(), Pos::new(0, 2));
    feed(&mut s, "d<c-n>"); // linewise, as dj
    assert_eq!(s.text(), "fghij");
    feed(&mut s, "/<c-p>x<esc>"); // nothing in the search prompt
    assert_eq!(s.text(), "fghij");
}

#[test]
fn counts_multiply() {
    let mut s = session("a b c d e f g h");
    feed(&mut s, "3w");
    assert_eq!(s.cursor(), Pos::new(0, 6));
    feed(&mut s, "2b");
    assert_eq!(s.cursor(), Pos::new(0, 2));
}

#[test]
fn x_and_counted_x() {
    let mut s = session("abcdef");
    feed(&mut s, "x");
    assert_eq!(s.text(), "bcdef");
    feed(&mut s, "3x");
    assert_eq!(s.text(), "ef");
    assert_eq!(s.cursor(), Pos::new(0, 0));
}

#[test]
fn dd_and_paste_linewise() {
    let mut s = session("one\ntwo\nthree");
    feed(&mut s, "dd");
    assert_eq!(s.text(), "two\nthree");
    assert_eq!(s.cursor(), Pos::new(0, 0));
    feed(&mut s, "p");
    assert_eq!(s.text(), "two\none\nthree");
    assert_eq!(s.cursor(), Pos::new(1, 0));
    feed(&mut s, "P");
    assert_eq!(s.text(), "two\none\none\nthree");
}

#[test]
fn dd_last_line() {
    let mut s = at("one\ntwo", 1, 0);
    feed(&mut s, "dd");
    assert_eq!(s.text(), "one");
    assert_eq!(s.cursor(), Pos::new(0, 0));
}

#[test]
fn two_dd_with_count() {
    let mut s = session("a\nb\nc\nd");
    feed(&mut s, "3dd");
    assert_eq!(s.text(), "d");
}

#[test]
fn dw_stays_on_line_at_eol_word() {
    // "dw" on the last word of a line stops at end of line.
    let mut s = at("foo bar\nbaz", 0, 4);
    feed(&mut s, "dw");
    assert_eq!(s.text(), "foo \nbaz");
}

#[test]
fn dw_mid_line() {
    let mut s = session("foo bar baz");
    feed(&mut s, "dw");
    assert_eq!(s.text(), "bar baz");
    feed(&mut s, "d2w");
    assert_eq!(s.text(), "");
}

#[test]
fn cw_acts_like_ce() {
    let mut s = session("hello world");
    feed(&mut s, "cw");
    assert_eq!(s.mode_label(), "insert");
    assert_eq!(s.text(), " world");
    type_ins(&mut s, "goodbye");
    feed(&mut s, "<esc>");
    assert_eq!(s.text(), "goodbye world");
    assert_eq!(s.cursor(), Pos::new(0, 6));
}

#[test]
fn de_and_d_dollar() {
    let mut s = session("foo bar baz");
    feed(&mut s, "de");
    assert_eq!(s.text(), " bar baz");
    feed(&mut s, "w");
    assert_eq!(s.cursor(), Pos::new(0, 1));
    feed(&mut s, "d$");
    assert_eq!(s.text(), " ");
}

#[test]
fn yy_and_p() {
    let mut s = session("alpha\nbeta");
    feed(&mut s, "yyp");
    assert_eq!(s.text(), "alpha\nalpha\nbeta");
    assert_eq!(s.cursor(), Pos::new(1, 0));
}

#[test]
fn register_is_shared_across_sessions() {
    // Each open document is its own Session; yanking in one buffer must
    // paste in another, the way Vim's unnamed register spans buffers.
    let mut a = session("alpha\nbeta");
    let mut b = session("one\ntwo");
    feed(&mut a, "yy");
    feed(&mut b, "p");
    assert_eq!(b.text(), "one\nalpha\ntwo");
    feed(&mut b, "yw");
    feed(&mut a, "j$p");
    assert_eq!(a.text(), "alpha\nbetaalpha");
}

#[test]
fn yank_word_and_paste_charwise() {
    let mut s = session("foo bar");
    feed(&mut s, "yw");
    assert_eq!(s.cursor(), Pos::new(0, 0));
    feed(&mut s, "$p");
    assert_eq!(s.text(), "foo barfoo ");
}

#[test]
fn charwise_paste_cursor_on_last_char() {
    let mut s = session("ab");
    feed(&mut s, "ylp"); // yank "a", paste after cursor
    assert_eq!(s.text(), "aab");
    assert_eq!(s.cursor(), Pos::new(0, 1));
}

#[test]
fn insert_entries() {
    let mut s = at("hello", 0, 2);
    feed(&mut s, "i");
    assert_eq!((s.mode_label(), s.cursor()), ("insert", Pos::new(0, 2)));
    feed(&mut s, "<esc>a");
    assert_eq!((s.mode_label(), s.cursor()), ("insert", Pos::new(0, 2)));
    feed(&mut s, "<esc>A");
    assert_eq!(s.cursor(), Pos::new(0, 5));
    feed(&mut s, "<esc>I");
    assert_eq!(s.cursor(), Pos::new(0, 0));
}

/// Simulate the host's line-insert command: the split plus whatever indent
/// the language configuration chose, delivered as an outside change.
fn host_open_line(s: &mut Session, line: usize, above: bool, indent: &str) {
    let text = s.text();
    let width = indent.encode_utf16().count();
    let at = if above {
        Pos::new(line, 0)
    } else {
        let len = text.lines().nth(line).map_or(0, |l| l.encode_utf16().count());
        Pos::new(line, len)
    };
    let inserted = if above {
        format!("{indent}\n")
    } else {
        format!("\n{indent}")
    };
    s.apply_change(at, at, &inserted);
    s.set_position(if above { line } else { line + 1 }, width);
}

#[test]
fn open_lines_leave_the_split_to_the_host() {
    let mut s = session("fn f() {\n}");
    let fx = feed(&mut s, "o");
    // No edit: only the host knows this language wants a deeper indent
    // inside the braces, so the split travels as its line-insert command.
    assert!(fx.edits.is_empty());
    assert!(matches!(fx.commands[..], [Command::OpenLine { above: false }]));
    assert_eq!((fx.mode, s.cursor()), ("insert", Pos::new(0, 0)));

    host_open_line(&mut s, 0, false, "    ");
    assert_eq!(s.text(), "fn f() {\n    \n}");
    assert_eq!((s.mode_label(), s.cursor()), ("insert", Pos::new(1, 4)));

    let fx = feed(&mut s, "<esc>O");
    assert!(matches!(fx.commands[..], [Command::OpenLine { above: true }]));
    host_open_line(&mut s, 1, true, "    ");
    assert_eq!(s.text(), "fn f() {\n    \n    \n}");
    assert_eq!((s.mode_label(), s.cursor()), ("insert", Pos::new(1, 4)));
}

#[test]
fn escape_moves_cursor_left() {
    let mut s = at("hello", 0, 2);
    feed(&mut s, "a<esc>");
    assert_eq!((s.mode_label(), s.cursor()), ("normal", Pos::new(0, 2)));
}

#[test]
fn insert_and_type() {
    let mut s = session("world");
    feed(&mut s, "i");
    type_ins(&mut s, "hello ");
    feed(&mut s, "<esc>");
    assert_eq!(s.text(), "hello world");
    assert_eq!(s.cursor(), Pos::new(0, 5));
}

#[test]
fn gg_and_g_with_counts() {
    let mut s = session("l1\nl2\nl3\nl4");
    feed(&mut s, "G");
    assert_eq!(s.cursor(), Pos::new(3, 0));
    feed(&mut s, "gg");
    assert_eq!(s.cursor(), Pos::new(0, 0));
    feed(&mut s, "3G");
    assert_eq!(s.cursor(), Pos::new(2, 0));
    feed(&mut s, "2gg");
    assert_eq!(s.cursor(), Pos::new(1, 0));
}

#[test]
fn gh_asks_the_host_for_a_hover() {
    let mut s = at("let x = foo();", 0, 8);
    let fx = feed(&mut s, "gh");
    assert!(matches!(fx.commands[..], [Command::ShowHover]));
    // The cursor stays where it is and nothing is pending afterwards.
    assert_eq!((fx.mode, s.cursor(), fx.pending.as_str()), ("normal", Pos::new(0, 8), ""));
    assert!(fx.edits.is_empty());
}

#[test]
fn d_gg_linewise() {
    let mut s = at("a\nb\nc", 1, 0);
    feed(&mut s, "dgg");
    assert_eq!(s.text(), "c");
}

#[test]
fn find_semicolon_comma() {
    let mut s = session("a.b.c.d");
    feed(&mut s, "f.");
    assert_eq!(s.cursor(), Pos::new(0, 1));
    feed(&mut s, ";");
    assert_eq!(s.cursor(), Pos::new(0, 3));
    feed(&mut s, ",");
    assert_eq!(s.cursor(), Pos::new(0, 1));
    feed(&mut s, "2;");
    assert_eq!(s.cursor(), Pos::new(0, 5));
}

#[test]
fn dt_and_df() {
    let mut s = session("foo(bar)");
    feed(&mut s, "dt(");
    assert_eq!(s.text(), "(bar)");
    let mut s = session("foo(bar)");
    feed(&mut s, "df(");
    assert_eq!(s.text(), "bar)");
}

#[test]
fn replace_char() {
    let mut s = session("abc");
    feed(&mut s, "rx");
    assert_eq!(s.text(), "xbc");
    assert_eq!(s.cursor(), Pos::new(0, 0));
    feed(&mut s, "3ry");
    assert_eq!(s.text(), "yyy");
    assert_eq!(s.cursor(), Pos::new(0, 2));
    // Count larger than what's left: fails, no change.
    feed(&mut s, "0" /* go to col 0 */);
    feed(&mut s, "9rz");
    assert_eq!(s.text(), "yyy");
}

#[test]
fn join_lines() {
    let mut s = session("foo\n  bar\nbaz");
    feed(&mut s, "J");
    assert_eq!(s.text(), "foo bar\nbaz");
    assert_eq!(s.cursor(), Pos::new(0, 3));
    let mut s = session("a\nb\nc\nd");
    feed(&mut s, "3J");
    assert_eq!(s.text(), "a b c\nd");
}

#[test]
fn tilde_toggles_case() {
    let mut s = session("aBc");
    feed(&mut s, "3~");
    assert_eq!(s.text(), "AbC");
    assert_eq!(s.cursor(), Pos::new(0, 2));
}

#[test]
fn visual_delete() {
    let mut s = session("hello world");
    feed(&mut s, "vllld");
    assert_eq!(s.text(), "o world");
    assert_eq!((s.mode_label(), s.cursor()), ("normal", Pos::new(0, 0)));
}

#[test]
fn visual_line_delete_and_paste() {
    let mut s = session("one\ntwo\nthree");
    feed(&mut s, "Vjd");
    assert_eq!(s.text(), "three");
    feed(&mut s, "p");
    assert_eq!(s.text(), "three\none\ntwo");
}

#[test]
fn visual_swap_ends() {
    let mut s = at("abcdef", 0, 3);
    feed(&mut s, "vll");
    assert_eq!(s.cursor(), Pos::new(0, 5));
    feed(&mut s, "o");
    assert_eq!(s.cursor(), Pos::new(0, 3));
    feed(&mut s, "hh");
    assert_eq!(s.cursor(), Pos::new(0, 1));
    feed(&mut s, "d");
    assert_eq!(s.text(), "a");
}

#[test]
fn visual_yank_puts_cursor_at_start() {
    let mut s = at("hello", 0, 1);
    feed(&mut s, "vlly");
    assert_eq!(s.cursor(), Pos::new(0, 1));
    feed(&mut s, "$p");
    assert_eq!(s.text(), "helloell");
}

#[test]
fn visual_change() {
    let mut s = session("hello world");
    feed(&mut s, "vec");
    assert_eq!(s.mode_label(), "insert");
    assert_eq!(s.text(), " world");
}

#[test]
fn visual_mode_toggles() {
    let mut s = session("ab\ncd");
    feed(&mut s, "v");
    assert_eq!(s.mode_label(), "visual");
    feed(&mut s, "V");
    assert_eq!(s.mode_label(), "visualLine");
    feed(&mut s, "V");
    assert_eq!(s.mode_label(), "normal");
    feed(&mut s, "v<esc>");
    assert_eq!(s.mode_label(), "normal");
}

#[test]
fn text_object_diw_daw() {
    let mut s = at("foo bar baz", 0, 5);
    feed(&mut s, "diw");
    assert_eq!(s.text(), "foo  baz");
    let mut s = at("foo bar baz", 0, 5);
    feed(&mut s, "daw");
    assert_eq!(s.text(), "foo baz");
}

#[test]
fn text_object_quotes_and_brackets() {
    let mut s = at(r#"say "hi there" now"#, 0, 7);
    feed(&mut s, "ci\"");
    assert_eq!(s.text(), r#"say "" now"#);
    assert_eq!(s.mode_label(), "insert");

    let mut s = at("f(a, g(b), c)", 0, 3);
    feed(&mut s, "di(");
    assert_eq!(s.text(), "f()");
    let mut s = at("f(a, g(b), c)", 0, 3);
    feed(&mut s, "da(");
    assert_eq!(s.text(), "f");
}

#[test]
fn visual_text_object() {
    let mut s = at("foo (bar baz) qux", 0, 6);
    feed(&mut s, "vi(d");
    assert_eq!(s.text(), "foo () qux");
}

/// `keys` at `(line, col)`: the text and cursor they leave. The expectations
/// in the object and sentence tests below are what Vim 9.1 (`vim -u NONE`)
/// does with the same text and keys.
fn after(text: &str, line: usize, col: usize, keys: &str) -> (String, Pos) {
    let mut s = at(text, line, col);
    feed(&mut s, keys);
    (s.text(), s.cursor())
}

fn p(line: usize, col: usize) -> Pos {
    Pos::new(line, col)
}

const PARAS: &str = "a\nb\n\nc\nd\n\n\ne";

#[test]
fn paragraph_objects_are_linewise() {
    // `ap` takes the blank lines after it, `ip` only the paragraph.
    assert_eq!(after(PARAS, 0, 0, "dap"), ("c\nd\n\n\ne".into(), p(0, 0)));
    assert_eq!(after(PARAS, 0, 0, "dip"), ("\nc\nd\n\n\ne".into(), p(0, 0)));
    assert_eq!(after(PARAS, 3, 0, "dap"), ("a\nb\n\ne".into(), p(3, 0)));
    // A run of blank lines is a paragraph of its own; its `ap` is the blank
    // lines and the paragraph after them.
    assert_eq!(after(PARAS, 2, 0, "dip").0, "a\nb\nc\nd\n\n\ne");
    assert_eq!(after(PARAS, 2, 0, "dap").0, "a\nb\n\n\ne");
    // The last paragraph has no blank lines after it, so `ap` takes the
    // ones before.
    assert_eq!(after(PARAS, 7, 0, "dap"), ("a\nb\n\nc\nd".into(), p(4, 0)));
    assert_eq!(after(PARAS, 7, 0, "dip"), ("a\nb\n\nc\nd\n\n".into(), p(6, 0)));
    // Lines of only blanks count as blank, unlike for `{` and `}`.
    assert_eq!(after("a\n  \nb", 0, 0, "dip").0, "  \nb");
    assert_eq!(after("a\n  \nb", 0, 0, "dap").0, "b");
    // `cip` keeps a line to type on; `yap` yanks lines and lands on the
    // first.
    let mut s = at(PARAS, 3, 0);
    feed(&mut s, "cip");
    assert_eq!((s.text().as_str(), s.mode_label()), ("a\nb\n\n\n\n\ne", "insert"));
    let mut s = at(PARAS, 4, 0);
    feed(&mut s, "yap");
    assert_eq!(s.cursor(), p(3, 0));
    feed(&mut s, "P");
    assert_eq!(s.text(), "a\nb\n\nc\nd\n\n\nc\nd\n\n\ne");
}

#[test]
fn paragraph_counts_and_visual() {
    // Counts take more paragraphs; for `ip` a blank run counts as one.
    assert_eq!(after(PARAS, 0, 0, "d2ap").0, "e");
    assert_eq!(after(PARAS, 0, 0, "d2ip").0, "c\nd\n\n\ne");
    // Past the last paragraph the object fails and nothing happens.
    assert_eq!(after(PARAS, 7, 0, "3dap"), (PARAS.into(), p(7, 0)));
    // In visual mode it turns the selection linewise, and repeating it grows
    // the selection.
    let mut s = at(PARAS, 0, 0);
    let fx = feed(&mut s, "vip");
    assert_eq!(fx.mode, "visualLine");
    feed(&mut s, "d");
    assert_eq!(s.text(), "\nc\nd\n\n\ne");
    assert_eq!(after(PARAS, 0, 0, "Vipipd").0, "c\nd\n\n\ne");
    assert_eq!(after(PARAS, 0, 0, "vapapd").0, "e");
    // `Vip` on a blank line that is a paragraph by itself would be stuck;
    // it grows instead.
    assert_eq!(after(PARAS, 5, 0, "Vipipd"), ("a\nb\n\nc\nd".into(), p(4, 0)));
    // A charwise selection across lines grows by paragraphs on the cursor's
    // side and stays charwise.
    let mut s = at(PARAS, 7, 0);
    let fx = feed(&mut s, "vkkip");
    assert_eq!(fx.mode, "visual");
    feed(&mut s, "d");
    assert_eq!(s.text(), "a\nb\n\n");
}

const SENTS: &str = "Hello world. Foo bar! Baz qux?  Last one";

#[test]
fn sentence_motions() {
    // `)` to the next sentence's start, past the blanks after `.!?`.
    assert_eq!(after(SENTS, 0, 0, ")D"), ("Hello world. ".into(), p(0, 12)));
    assert_eq!(after(SENTS, 0, 0, "2)D"), ("Hello world. Foo bar! ".into(), p(0, 21)));
    // The last sentence runs to the end of the buffer; a count past that
    // fails.
    assert_eq!(after(SENTS, 0, 0, "4)D").0, "Hello world. Foo bar! Baz qux?  Last on");
    let mut s = at(SENTS, 0, 0);
    let fx = feed(&mut s, "5)");
    assert_eq!((s.cursor(), fx.pending.as_str()), (p(0, 0), ""));
    // `(` to this sentence's start, then the ones before.
    assert_eq!(after(SENTS, 0, 19, "(D").0, "Hello world. ");
    assert_eq!(after(SENTS, 0, 19, "2(D").0, "");
    // Exclusive with an operator.
    let deleted = "Hello world. Foo baBaz qux?  Last one";
    assert_eq!(after(SENTS, 0, 19, "d)"), (deleted.into(), p(0, 19)));
    assert_eq!(after(SENTS, 0, 19, "d("), ("Hello world. r! Baz qux?  Last one".into(), p(0, 13)));
    // Closing quotes and brackets after the `.` belong to the sentence.
    let quoted = r#"He said "Stop." Then (left.) Done"#;
    assert_eq!(after(quoted, 0, 0, ")D").0, r#"He said "Stop." "#);
    assert_eq!(after(quoted, 0, 0, "2)D").0, r#"He said "Stop." Then (left.) "#);
    // An empty line is a sentence of its own.
    let paras = "a. b\n\nc. d";
    assert_eq!(after(paras, 0, 3, ")").1, p(1, 0));
    assert_eq!(after(paras, 0, 3, "2)").1, p(2, 0));
    assert_eq!(after(paras, 2, 0, "(").1, p(1, 0));
    assert_eq!(after(paras, 2, 0, "2(").1, p(0, 3));
    // Across lines; `d)` up to a sentence opening the next line leaves the
    // line break (`:help exclusive-linewise`).
    let two = "One. Two.\nThree four.";
    assert_eq!(after(two, 0, 0, "3)D"), ("One. Two.\nThree four".into(), p(1, 9)));
    assert_eq!(after(two, 0, 5, "d)"), ("One. \nThree four.".into(), p(0, 4)));
    // Visual mode extends to the sentence start, and `y)` yanks up to it.
    assert_eq!(after(SENTS, 0, 14, "v)d").0, "Hello world. Faz qux?  Last one");
    let pasted = "Hello world. Foo bar! Hello world. Foo bar! Baz qux?  Last one";
    assert_eq!(after(SENTS, 0, 0, "y2)P").0, pasted);
}

#[test]
fn sentence_objects() {
    // `as` takes the blanks after the sentence, `is` doesn't.
    assert_eq!(after(SENTS, 0, 14, "das"), ("Hello world. Baz qux?  Last one".into(), p(0, 13)));
    assert_eq!(after(SENTS, 0, 14, "dis").0, "Hello world.  Baz qux?  Last one");
    // Counts: for `is` the blanks between sentences count as sentences too.
    assert_eq!(after(SENTS, 0, 14, "d2as").0, "Hello world. Last one");
    assert_eq!(after(SENTS, 0, 14, "d2is").0, "Hello world. Baz qux?  Last one");
    assert_eq!(after(SENTS, 0, 14, "d3is").0, "Hello world.   Last one");
    // The last sentence has no blanks after it: `as` takes the ones before.
    assert_eq!(after(SENTS, 0, 35, "das"), ("Hello world. Foo bar! Baz qux?".into(), p(0, 29)));
    // On the blanks between sentences, `is` is the blanks and `as` the
    // blanks and the sentence after them.
    assert_eq!(after(SENTS, 0, 12, "dis").0, "Hello world.Foo bar! Baz qux?  Last one");
    assert_eq!(after(SENTS, 0, 12, "das"), ("Hello world. Baz qux?  Last one".into(), p(0, 12)));
    assert_eq!(after(SENTS, 0, 30, "dis").0, "Hello world. Foo bar! Baz qux?Last one");
    assert_eq!(after(SENTS, 0, 30, "das").0, "Hello world. Foo bar! Baz qux?");
    let quoted = r#"He said "Stop." Then (left.) Done"#;
    assert_eq!(after(quoted, 0, 22, "dis").0, r#"He said "Stop."  Done"#);
    // A line's last sentence keeps its line break; the whole of a line's
    // only sentence leaves an empty line.
    let two = "One. Two.\nThree four.";
    assert_eq!(after(two, 0, 5, "das"), ("One.\nThree four.".into(), p(0, 3)));
    assert_eq!(after(two, 1, 0, "das"), ("One. Two.\n".into(), p(1, 0)));
    // In visual mode a repeat grows the selection, toward the cursor's side.
    assert_eq!(after(SENTS, 0, 14, "visisd").0, "Hello world. Baz qux?  Last one");
    assert_eq!(after(SENTS, 0, 14, "vasasd").0, "Hello world. Last one");
    let grown = "Hello world.o bar! Baz qux?  Last one";
    assert_eq!(after(SENTS, 0, 14, "vhisd"), (grown.into(), p(0, 12)));
}

#[test]
fn word_object_counts_and_blanks() {
    let words = "one two three four";
    // `aw` counts words, its blanks riding along; `iw` counts blank runs as
    // words.
    assert_eq!(after(words, 0, 0, "d2aw").0, "three four");
    assert_eq!(after(words, 0, 0, "d3iw").0, " three four");
    assert_eq!(after(words, 0, 4, "d2iw").0, "one three four");
    // The last word on a line has no blanks after it: `aw` takes the ones
    // before, but never a line's indent.
    assert_eq!(after(words, 0, 15, "daw"), ("one two three".into(), p(0, 12)));
    assert_eq!(after("foo bar\nbaz", 0, 4, "daw"), ("foo\nbaz".into(), p(0, 2)));
    assert_eq!(after("foo bar.", 0, 4, "daw").0, "foo.");
    assert_eq!(after("  foo bar", 0, 2, "daw").0, "  bar");
    // Running out of words fails the whole object.
    assert_eq!(after(words, 0, 15, "d2aw").0, words);
    assert_eq!(after(words, 0, 8, "d2aw"), ("one two".into(), p(0, 6)));
    // On blanks, `aw` is the blanks and the word after them.
    assert_eq!(after("foo   bar", 0, 4, "daw").0, "foo");
    assert_eq!(after("foo   bar", 0, 4, "diw").0, "foobar");
    // A count crosses lines.
    assert_eq!(after("one\ntwo three", 0, 0, "d2aw").0, "three");
    // On an empty line `iw` is nothing, and `aw` the line break and the word
    // after it, which a delete makes whole lines.
    assert_eq!(after("one two\n\nthree", 1, 0, "diw").0, "one two\n\nthree");
    assert_eq!(after("one two\n\nthree", 1, 0, "daw").0, "one two");
}

#[test]
fn word_object_grows_a_visual_selection() {
    let words = "one two three four";
    assert_eq!(after(words, 0, 4, "vawawd").0, "one four");
    assert_eq!(after(words, 0, 4, "viwiwiwd").0, "one  four");
    assert_eq!(after(words, 0, 8, "v2awd").0, "one two");
    // With the cursor at the selection's start it grows backward.
    assert_eq!(after(words, 0, 8, "vawhawd"), ("one two ".into(), p(0, 7)));
    // Linewise visual becomes charwise.
    let mut s = at(words, 0, 4);
    let fx = feed(&mut s, "Vaw");
    assert_eq!(fx.mode, "visual");
    feed(&mut s, "d");
    assert_eq!(s.text(), "one three four");
}

#[test]
fn bracket_objects() {
    let call = "f(a, g(b), c)";
    // A count goes out that many levels; past the outermost it fails.
    assert_eq!(after(call, 0, 7, "d2i(").0, "f()");
    assert_eq!(after(call, 0, 7, "d2a(").0, "f");
    assert_eq!(after(call, 0, 7, "d3i(").0, call);
    // On either bracket, that pair.
    assert_eq!(after(call, 0, 1, "di(").0, "f()");
    assert_eq!(after(call, 0, 12, "di(").0, "f()");
    // Outside any block, the next one ahead — on later lines too.
    assert_eq!(after("x = 1; f(a) g(b)", 0, 0, "di("), ("x = 1; f() g(b)".into(), p(0, 9)));
    assert_eq!(after("x = 1;\nf(a)", 0, 0, "di(").0, "x = 1;\nf()");
    // A yank leaves the cursor on the object's start, even ahead of it.
    assert_eq!(after("x = 1; f(a)", 0, 0, "yi(").1, p(0, 9));
    assert_eq!(after("<a>x</a>", 0, 1, "yit").1, p(0, 3));
    // An empty block is an empty range: `ci(` types into it.
    let mut s = at("f()", 0, 1);
    feed(&mut s, "ci(");
    assert_eq!((s.mode_label(), s.cursor()), ("insert", p(0, 2)));
    type_ins(&mut s, "X");
    assert_eq!(s.text(), "f(X)");
    // The search for the closing bracket minds strings, the one for the
    // opening bracket doesn't: from the `x` this finds the `(` in the string
    // and no `)` for it.
    assert_eq!(after(r#"s("(", x)"#, 0, 7, "di(").0, r#"s("(", x)"#);
    // In visual mode a repeat goes out a level.
    assert_eq!(after("f(a, (bc)) x", 0, 6, "vi(i(d").0, "f() x");
    assert_eq!(after("f(a, (bc)) x", 0, 6, "va(a(d").0, "f x");
    assert_eq!(after(call, 0, 7, "va(a(d").0, "f");
}

#[test]
fn inner_block_leaves_out_the_brace_lines() {
    // The braces' own lines stay, and the lines between go whole.
    let block = "if x {\n  a\n  b\n}";
    assert_eq!(after(block, 1, 2, "di{"), ("if x {\n}".into(), p(1, 0)));
    assert_eq!(after(block, 1, 2, "da{"), ("if x ".into(), p(0, 4)));
    assert_eq!(after("if x {\n  a\n  b\n  }", 1, 2, "di{").0, "if x {\n  }");
    let mut s = at(block, 1, 2);
    feed(&mut s, "ci{");
    type_ins(&mut s, "X");
    assert_eq!(s.text(), "if x {\n  X\n}");
    let mut s = at("if x {\n  a\n}", 1, 2);
    feed(&mut s, "yi{P");
    assert_eq!(s.text(), "if x {\n  a\n  a\n}");
    // Text beside a brace stays charwise.
    assert_eq!(after("if x { a\n  b }", 0, 7, "di{").0, "if x {}");
    // Nested, with a count, and from a brace line's indent.
    assert_eq!(after("{\n  a\n  {\n    b\n  }\n}", 3, 4, "d2i{").0, "{\n}");
    assert_eq!(after("  {\n    b\n  }", 0, 0, "di{").0, "  {\n  }");
}

const TAGS: &str = "<div><p>hi</p></div>";

#[test]
fn tag_objects() {
    assert_eq!(after(TAGS, 0, 8, "dit"), ("<div><p></p></div>".into(), p(0, 8)));
    assert_eq!(after(TAGS, 0, 8, "dat"), ("<div></div>".into(), p(0, 5)));
    // A count goes out that many tags.
    assert_eq!(after(TAGS, 0, 8, "d2it").0, "<div></div>");
    assert_eq!(after(TAGS, 0, 8, "d2at").0, "");
    assert_eq!(after(TAGS, 0, 8, "d3it").0, TAGS);
    // On a tag, that tag's block.
    assert_eq!(after(TAGS, 0, 5, "dit").0, "<div><p></p></div>");
    assert_eq!(after(TAGS, 0, 10, "dit").0, "<div><p></p></div>");
    assert_eq!(after(TAGS, 0, 12, "dit").0, "<div><p></p></div>");
    assert_eq!(after(TAGS, 0, 1, "dit").0, "<div></div>");
    // A tag with no end tag and a self-closing one are passed over.
    assert_eq!(after("<div><br><p>hi</p> x</div>", 0, 23, "dit").0, "<div></div>");
    assert_eq!(after(r#"<div><br/>x <img src="a" /> y</div>"#, 0, 11, "dit").0, "<div></div>");
    // Case is ignored, a `>` in quotes doesn't end the tag, and a stray end
    // tag is ignored.
    assert_eq!(after("<B>x</b>", 0, 3, "dit").0, "<B></b>");
    assert_eq!(after(r#"<a href="x>y">t</a>"#, 0, 15, "dit").0, r#"<a href="x>y"></a>"#);
    assert_eq!(after("</p> <p>a</p>", 0, 9, "dit").0, "</p> <p></p>");
    // Attributes on the next line.
    assert_eq!(after("<div\n  class=\"x\">a</div>", 1, 10, "dit").0, "<div\n  class=\"x\"></div>");
    // An empty block: `cit` types between the tags.
    let mut s = at("<b></b>", 0, 1);
    feed(&mut s, "cit");
    assert_eq!((s.mode_label(), s.cursor()), ("insert", p(0, 3)));
}

#[test]
fn tag_objects_across_lines_and_in_visual() {
    // `it` over lines takes the line breaks inside the tags too, indented
    // end tag or not.
    assert_eq!(after("<b>\n  foo\n  </b>", 1, 2, "dit"), ("<b></b>".into(), p(0, 3)));
    assert_eq!(after("<b>\nfoo\n</b>", 1, 0, "dit").0, "<b></b>");
    assert_eq!(after("<b>\nfoo\n</b>", 1, 0, "dat").0, "");
    let list = "<ul>\n  <li>a</li>\n  <li>b</li>\n</ul>";
    assert_eq!(after(list, 2, 6, "dit").0, "<ul>\n  <li>a</li>\n  <li></li>\n</ul>");
    assert_eq!(after(list, 2, 0, "dit").0, "<ul>\n  <li>a</li>\n  <li></li>\n</ul>");
    assert_eq!(after(list, 2, 6, "d2it").0, "<ul></ul>");
    // In visual mode, `it` again takes the tags, then the next block out.
    assert_eq!(after(TAGS, 0, 8, "vitd").0, "<div><p></p></div>");
    assert_eq!(after(TAGS, 0, 8, "vititd").0, "<div></div>");
    assert_eq!(after(TAGS, 0, 8, "vatatd").0, "");
}

#[test]
fn quote_objects() {
    let say = r#"say "hi there" now"#;
    assert_eq!(after(say, 0, 7, "di\""), (r#"say "" now"#.into(), p(0, 5)));
    // Before the first quote on the line: the first string.
    assert_eq!(after(say, 0, 0, "di\"").0, r#"say "" now"#);
    // On a quote, the line's quotes pair up from its start to say which
    // string it is; between two strings, the text between them is taken for
    // one, as in Vim.
    let two = r#"x "a" y "b""#;
    assert_eq!(after(two, 0, 2, "di\"").0, r#"x "" y "b""#);
    assert_eq!(after(two, 0, 4, "di\"").0, r#"x "" y "b""#);
    assert_eq!(after(two, 0, 8, "di\"").0, r#"x "a" y """#);
    assert_eq!(after(two, 0, 10, "di\"").0, r#"x "a" y """#);
    assert_eq!(after(two, 0, 6, "di\""), (r#"x "a""b""#.into(), p(0, 5)));
    // `a"` takes the blanks after, or with none the blanks before.
    assert_eq!(after(r#"a "b" c"#, 0, 3, "da\"").0, "a c");
    assert_eq!(after(r#"a "b""#, 0, 3, "da\"").0, "a");
    assert_eq!(after(r#"a "b","#, 0, 3, "da\"").0, "a,");
    assert_eq!(after(r#""abc""#, 0, 2, "da\"").0, "");
    // `2i"` takes the quotes but no blanks.
    assert_eq!(after(say, 0, 7, "d2i\"").0, "say  now");
    // `\` escapes a quote.
    let esc = r#"say "hi \"x\" there" now"#;
    assert_eq!(after(esc, 0, 7, "di\"").0, r#"say "" now"#);
    assert_eq!(after(esc, 0, 11, "di\"").0, r#"say "" now"#);
    let mut s = at("say 'it' now", 0, 6);
    feed(&mut s, "ci'");
    type_ins(&mut s, "X");
    assert_eq!(s.text(), "say 'X' now");
    // An empty string: `ci"` types into it, `vi"` takes both quotes.
    let mut s = at(r#"x = "" y"#, 0, 5);
    feed(&mut s, "ci\"");
    assert_eq!((s.mode_label(), s.cursor()), ("insert", p(0, 5)));
    assert_eq!(after(r#"x = "" y"#, 0, 5, "vi\"d").0, "x =  y");
    // In visual mode `i"` again takes the quotes, `a"` the next string.
    assert_eq!(after(say, 0, 7, "vi\"i\"d").0, "say  now");
    assert_eq!(after(say, 0, 7, "va\"d").0, "say now");
    assert_eq!(after(r#"say "a" and "b" now"#, 0, 5, "va\"a\"d").0, "say now");
}

#[test]
fn objects_at_every_cursor() {
    let mut s = session("f(a) g(b)\nh(c) i(d)");
    cursors(&mut s, &[(0, 2), (1, 7)]);
    feed(&mut s, "di(");
    assert_eq!(s.text(), "f() g(b)\nh(c) i()");
    let mut s = session("One. Two.\nThree. Four.");
    cursors(&mut s, &[(0, 0), (1, 0)]);
    feed(&mut s, "das");
    assert_eq!(s.text(), "Two.\nFour.");
    let mut s = session("One. Two.\nThree. Four.");
    cursors(&mut s, &[(0, 0), (1, 0)]);
    let fx = feed(&mut s, ")");
    assert_eq!(actives(&fx), [(0, 5), (1, 7)]);
    let mut s = session("<a>x</a>\n<b>y</b>");
    cursors(&mut s, &[(0, 3), (1, 3)]);
    feed(&mut s, "cit");
    assert_eq!((s.text().as_str(), s.mode_label()), ("<a></a>\n<b></b>", "insert"));
    // `vip` at two cursors in two paragraphs: both selections go linewise.
    let mut s = session("a\nb\n\nc\nd");
    cursors(&mut s, &[(0, 0), (3, 0)]);
    let fx = feed(&mut s, "vip");
    assert_eq!(fx.mode, "visualLine");
    feed(&mut s, "d");
    assert_eq!(s.text(), "");
}

#[test]
fn percent_motion_and_d_percent() {
    let mut s = session("(abc)def");
    feed(&mut s, "%");
    assert_eq!(s.cursor(), Pos::new(0, 4));
    feed(&mut s, "d%");
    assert_eq!(s.text(), "def");
}

#[test]
fn unmatched_bracket_motions() {
    let mut s = at("fn f() {\n  if (a) {\n    x\n  }\n}\n", 2, 4);
    // `]}` climbs out of the block the cursor is in, one press per level.
    feed(&mut s, "]}");
    assert_eq!(s.cursor(), Pos::new(3, 2));
    feed(&mut s, "]}");
    assert_eq!(s.cursor(), Pos::new(4, 0));
    // `[{` climbs the other way, and takes a count.
    let mut s = at("fn f() {\n  if (a) {\n    x\n  }\n}\n", 2, 4);
    feed(&mut s, "[{");
    assert_eq!(s.cursor(), Pos::new(1, 9));
    feed(&mut s, "[{");
    assert_eq!(s.cursor(), Pos::new(0, 7));
    let mut s = at("fn f() {\n  if (a) {\n    x\n  }\n}\n", 2, 4);
    feed(&mut s, "2[{");
    assert_eq!(s.cursor(), Pos::new(0, 7));
    // `[(` / `])` on the same line.
    let mut s = at("fn f(a, g(b), c) x", 0, 11);
    feed(&mut s, "[(");
    assert_eq!(s.cursor(), Pos::new(0, 9));
    feed(&mut s, "])");
    assert_eq!(s.cursor(), Pos::new(0, 11));
    // Nothing unmatched that way: the cursor stays put and the keys clear.
    let fx = feed(&mut s, "[{");
    assert_eq!(s.cursor(), Pos::new(0, 11));
    assert_eq!(fx.pending, "");
    // `]` with a bracket that doesn't face that way is not a motion.
    feed(&mut s, "]{");
    assert_eq!(s.cursor(), Pos::new(0, 11));
}

#[test]
fn unmatched_bracket_with_operator_and_visual() {
    // Exclusive, like vim: the brace itself survives the operator.
    let mut s = at("{ a b }", 0, 3);
    feed(&mut s, "d]}");
    assert_eq!(s.text(), "{ a}");
    let mut s = at("{ a b }", 0, 3);
    feed(&mut s, "d[{");
    assert_eq!(s.text(), " b }");
    // Visual mode covers the char the cursor lands on, brace included.
    let mut s = at("{ a b }", 0, 3);
    feed(&mut s, "v]}d");
    assert_eq!(s.text(), "{ a");
}

#[test]
fn paragraph_motions() {
    let mut s = session("a\nb\n\nc\n\nd");
    feed(&mut s, "}");
    assert_eq!(s.cursor(), Pos::new(2, 0));
    feed(&mut s, "}");
    assert_eq!(s.cursor(), Pos::new(4, 0));
    feed(&mut s, "{{");
    assert_eq!(s.cursor(), Pos::new(0, 0));
}

#[test]
fn undo_redo_emit_commands() {
    let mut s = session("abc");
    let fx = s.key(Key::Char('u'));
    assert!(matches!(fx.commands[..], [Command::Undo]));
    let fx = s.key(Key::Ctrl('r'));
    assert!(matches!(fx.commands[..], [Command::Redo]));
}

#[test]
fn indent_emits_command() {
    let mut s = session("a\nb\nc");
    let mut got = Vec::new();
    feed(&mut s, ">");
    let fx = s.key(Key::Char('>'));
    got.extend(fx.commands);
    assert!(matches!(
        got[..],
        [Command::IndentLines { start_line: 0, end_line: 0, dedent: false, times: 1 }]
    ));
}

#[test]
fn d_and_c_to_eol() {
    let mut s = at("hello world", 0, 5);
    feed(&mut s, "D");
    assert_eq!(s.text(), "hello");
    assert_eq!(s.cursor(), Pos::new(0, 4));
    let mut s = at("hello world", 0, 5);
    feed(&mut s, "C");
    assert_eq!((s.mode_label(), s.text().as_str()), ("insert", "hello"));
}

#[test]
fn s_and_big_s() {
    let mut s = session("abc");
    feed(&mut s, "2s");
    assert_eq!((s.mode_label(), s.text().as_str()), ("insert", "c"));
    let mut s = at("  indented", 0, 5);
    feed(&mut s, "S");
    assert_eq!((s.mode_label(), s.text().as_str()), ("insert", "  "));
    assert_eq!(s.cursor(), Pos::new(0, 2));
}

#[test]
fn zero_and_caret() {
    let mut s = at("   hello", 0, 7);
    feed(&mut s, "0");
    assert_eq!(s.cursor(), Pos::new(0, 0));
    feed(&mut s, "$^");
    assert_eq!(s.cursor(), Pos::new(0, 3));
}

#[test]
fn pending_count_then_escape() {
    let mut s = session("abcdef");
    let fx = s.key(Key::Char('2'));
    assert_eq!(fx.pending, "2");
    let fx = s.key(Key::Char('d'));
    assert_eq!(fx.pending, "2d");
    let fx = s.key(Key::Esc);
    assert_eq!(fx.pending, "");
    assert_eq!(s.text(), "abcdef");
}

#[test]
fn operator_aborts_on_non_motion() {
    let mut s = session("abc");
    feed(&mut s, "dp");
    assert_eq!(s.text(), "abc");
    assert_eq!(s.mode_label(), "normal");
}

#[test]
fn external_change_mirroring() {
    let mut s = session("hello world");
    // Host reports an external edit: someone replaced "world" with "there".
    s.apply_change(Pos::new(0, 6), Pos::new(0, 11), "there");
    assert_eq!(s.text(), "hello there");
    feed(&mut s, "$x");
    assert_eq!(s.text(), "hello ther");
}

#[test]
fn mouse_drag_enters_visual() {
    let mut s = session("hello world");
    let fx = s.set_selection(Pos::new(0, 0), Pos::new(0, 5), true);
    assert_eq!(fx.mode, "visual");
    feed(&mut s, "d");
    assert_eq!(s.text(), " world");
}

#[test]
fn a_commands_selection_does_not_enter_visual() {
    // cmd+f, enter, escape: the find widget leaves its match selected. That
    // highlight is not a visual selection — the `j` after it has to move the
    // cursor, not drag a selection along.
    let mut s = session("hello world\nsecond line");
    let fx = s.set_selection(Pos::new(0, 6), Pos::new(0, 11), false);
    assert_eq!(fx.mode, "normal");
    assert!(fx.selections.is_empty()); // the host keeps its highlight
    let fx = feed(&mut s, "j");
    assert_eq!(fx.mode, "normal");
    assert_eq!(actives(&fx), [(1, 10)]);
    assert_eq!(fx.selections[0].anchor, fx.selections[0].active); // collapsed

    // And searching from visual mode ends it, the way a command that moves
    // the cursor does.
    feed(&mut s, "v");
    let fx = s.set_selection(Pos::new(0, 6), Pos::new(0, 11), false);
    assert_eq!(fx.mode, "normal");
}

#[test]
fn selection_while_inserting_stays_in_insert() {
    // shift+right in insert mode: the host reports a selection with a body,
    // which must not end the insert the way a mouse drag from normal mode
    // starts visual.
    let mut s = at("hello world", 0, 5);
    feed(&mut s, "i");
    let fx = s.set_selection(Pos::new(0, 5), Pos::new(0, 8), true);
    assert_eq!(fx.mode, "insert");
    assert_eq!(s.cursor(), Pos::new(0, 8));
    // Nothing sent back: the host keeps the selection it just made.
    assert!(fx.selections.is_empty());
    // And the mode is still the one <esc> leaves.
    let fx = feed(&mut s, "<esc>");
    assert_eq!(fx.mode, "normal");
    assert_eq!(s.cursor(), Pos::new(0, 7));
}

#[test]
fn completion_placeholder_stays_in_insert() {
    // Accepting a suggestion: the host replaces the typed word and selects
    // the snippet placeholder it landed on. Still insert.
    let mut s = at("con", 0, 3);
    feed(&mut s, "a");
    s.apply_change(Pos::new(0, 0), Pos::new(0, 3), "concat(sep)");
    let fx = s.set_selection(Pos::new(0, 7), Pos::new(0, 10), false);
    assert_eq!(fx.mode, "insert");
    assert_eq!(s.text(), "concat(sep)");
    // Typing over the placeholder is the host's business; the mode survives.
    assert_eq!(s.mode_label(), "insert");
}

#[test]
fn multi_selection_while_inserting_stays_in_insert() {
    // Two placeholders of the same snippet, or shift+arrow with several
    // cursors: bodies at every cursor, still insert.
    let mut s = at("aa\naa", 0, 0);
    feed(&mut s, "i");
    let fx = s.set_cursors(
        &[
            (Pos::new(0, 0), Pos::new(0, 2)),
            (Pos::new(1, 0), Pos::new(1, 2)),
        ],
        true,
    );
    assert_eq!(fx.mode, "insert");
    assert!(fx.selections.is_empty());
    assert_eq!(s.cursor(), Pos::new(0, 2));
    let fx = feed(&mut s, "<esc>");
    assert_eq!(fx.mode, "normal");
    assert_eq!(actives(&fx), [(0, 1), (1, 1)]);
}

#[test]
fn backwards_drag_keeps_the_last_character() {
    // Dragging a line end-to-start. The press lands past the last character,
    // where normal mode won't let the caret stay, so the click that opens the
    // drag comes back clamped onto the last character — and the drag's anchor
    // with it. Reading that anchor as the far end of the selection is what
    // used to drop the last character of the line.
    let mut s = session("hello world");
    let fx = s.set_cursors(&[(Pos::new(0, 11), Pos::new(0, 11))], true);
    assert_eq!(actives(&fx), [(0, 10)]);
    let fx = s.set_selection(Pos::new(0, 10), Pos::new(0, 0), true);
    assert_eq!(fx.mode, "visual");
    // The host's highlight stopped one short of the 'd'; hand back the one
    // that covers it, so what is painted is what `d` deletes.
    assert_eq!(actives(&fx), [(0, 0)]);
    assert_eq!(fx.selections[0].anchor, Pos::new(0, 11));
    feed(&mut s, "d");
    assert_eq!(s.text(), "");
}

#[test]
fn backwards_drag_past_the_line_end_is_left_alone() {
    // The widened anchor comes back to the host, which reports it on the next
    // drag event: the engine must land on the same place and stop correcting.
    let mut s = session("hello world");
    let fx = s.set_selection(Pos::new(0, 11), Pos::new(0, 4), true);
    assert_eq!(fx.mode, "visual");
    assert!(fx.selections.is_empty()); // the host's highlight is already right
    feed(&mut s, "d");
    assert_eq!(s.text(), "hell");
}

#[test]
fn backwards_drag_mid_line_matches_the_highlight() {
    // Nowhere near the end of the line, the host's anchor really is one past
    // the last character it covers — no widening.
    let mut s = session("hello world");
    let fx = s.set_selection(Pos::new(0, 6), Pos::new(0, 0), true);
    assert_eq!(fx.mode, "visual");
    assert!(fx.selections.is_empty());
    feed(&mut s, "d");
    assert_eq!(s.text(), "world");
}

#[test]
fn backwards_drag_across_lines_keeps_the_line_it_started_on() {
    // Same drag from the end of the second line up to the first: the anchor
    // sits on that line's last character.
    let mut s = session("one\ntwo\nthree");
    let fx = s.set_selection(Pos::new(1, 2), Pos::new(0, 1), true);
    assert_eq!(fx.mode, "visual");
    assert_eq!(fx.selections[0].anchor, Pos::new(1, 3));
    feed(&mut s, "d");
    assert_eq!(s.text(), "o\nthree");
}

#[test]
fn set_position_clamps_in_normal_mode() {
    let mut s = session("abc");
    let fx = s.set_position(0, 3); // vscode allows col==len; vim clamps
    assert_eq!(s.cursor(), Pos::new(0, 2));
    assert_eq!(fx.selections.len(), 1);
}

#[test]
fn utf16_wide_chars() {
    let mut s = session("a😀b");
    feed(&mut s, "x");
    assert_eq!(s.text(), "😀b");
    feed(&mut s, "x");
    assert_eq!(s.text(), "b");
    let mut s = session("a😀b");
    feed(&mut s, "lx"); // cursor on emoji (col 1, width 2)
    assert_eq!(s.text(), "ab");
    assert_eq!(s.cursor(), Pos::new(0, 1));
}

#[test]
fn visual_replace() {
    let mut s = session("hello");
    feed(&mut s, "vllrx");
    assert_eq!(s.text(), "xxxlo");
    assert_eq!(s.mode_label(), "normal");
}

#[test]
fn visual_paste_over_selection() {
    let mut s = session("foo bar");
    feed(&mut s, "yw"); // register: "foo "
    feed(&mut s, "wv$p"); // select "bar", paste over
    assert_eq!(s.text(), "foo foo ");
}

#[test]
fn change_linewise_keeps_line() {
    let mut s = session("one\ntwo\nthree");
    feed(&mut s, "cc");
    assert_eq!(s.text(), "\ntwo\nthree");
    assert_eq!((s.mode_label(), s.cursor()), ("insert", Pos::new(0, 0)));
    // c2c == 2cc: blank the next two lines into one.
    feed(&mut s, "<esc>jc2c");
    assert_eq!(s.text(), "\n");
    assert_eq!(s.mode_label(), "insert");
    // The first line's indent stays, as 'autoindent' keeps it.
    let mut s = at("{\n\t  one\n    two\n}", 1, 4);
    feed(&mut s, "2cc");
    assert_eq!(s.text(), "{\n\t  \n}");
    assert_eq!((s.mode_label(), s.cursor()), ("insert", Pos::new(1, 3)));
    // `.` keeps the indent of the line it is on.
    let mut s = at("    a\n  b", 0, 0);
    feed(&mut s, "cc");
    type_ins(&mut s, "x");
    feed(&mut s, "<esc>j.");
    assert_eq!(s.text(), "    x\n  x");
}

#[test]
fn dj_deletes_two_lines() {
    let mut s = session("a\nb\nc");
    feed(&mut s, "dj");
    assert_eq!(s.text(), "c");
}

#[test]
fn search_forward_and_backward() {
    let mut s = session("foo bar\nbaz bar\nqux");
    feed(&mut s, "/bar<cr>");
    assert_eq!(s.cursor(), Pos::new(0, 4));
    feed(&mut s, "/bar<cr>");
    assert_eq!(s.cursor(), Pos::new(1, 4));
    feed(&mut s, "/bar<cr>"); // wraps
    assert_eq!(s.cursor(), Pos::new(0, 4));
    feed(&mut s, "?baz<cr>");
    assert_eq!(s.cursor(), Pos::new(1, 0));
    // A missing pattern leaves the cursor put.
    feed(&mut s, "/nope<cr>");
    assert_eq!(s.cursor(), Pos::new(1, 0));
}

#[test]
fn search_prompt_is_editable_and_cancelable() {
    let mut s = session("alpha beta");
    let fx = s.key(Key::Char('/'));
    assert_eq!(fx.pending, "/");
    feed(&mut s, "bex");
    let fx = s.key(Key::Char('t'));
    assert_eq!(fx.pending, "/bext");
    let fx = s.key(Key::Backspace); // fix the typo
    assert_eq!(fx.pending, "/bex");
    feed(&mut s, "<bs>t<cr>");
    assert_eq!(s.cursor(), Pos::new(0, 6));
    // Escape abandons the pattern; the buffer is untouched.
    feed(&mut s, "0/alpha<esc>");
    assert_eq!((s.cursor(), s.text().as_str()), (Pos::new(0, 0), "alpha beta"));
    // Backspacing past the prompt abandons it too.
    let fx = s.key(Key::Char('/'));
    assert_eq!(fx.pending, "/");
    let fx = s.key(Key::Backspace);
    assert_eq!(fx.pending, "");
}

#[test]
fn search_takes_a_count_and_an_empty_pattern_repeats() {
    let mut s = session("x a x a x a x");
    feed(&mut s, "3/a<cr>");
    assert_eq!(s.cursor(), Pos::new(0, 10));
    feed(&mut s, "0/<cr>"); // empty pattern reuses "a"
    assert_eq!(s.cursor(), Pos::new(0, 2));
}

#[test]
fn n_and_capital_n_repeat_the_search() {
    let mut s = session("one two\none three\none four");
    feed(&mut s, "/one<cr>");
    assert_eq!(s.cursor(), Pos::new(1, 0));
    feed(&mut s, "n");
    assert_eq!(s.cursor(), Pos::new(2, 0));
    feed(&mut s, "N");
    assert_eq!(s.cursor(), Pos::new(1, 0));
    feed(&mut s, "2n");
    assert_eq!(s.cursor(), Pos::new(0, 0));
    // After `?`, `n` keeps going backward and `N` reverses.
    feed(&mut s, "?three<cr>");
    assert_eq!(s.cursor(), Pos::new(1, 4));
    feed(&mut s, "N");
    assert_eq!(s.cursor(), Pos::new(1, 4)); // only one match: wraps to itself
}

#[test]
fn search_reports_match_rank_and_total() {
    let mut s = session("foo bar\nbaz bar\nqux");
    let fx = feed(&mut s, "/bar<cr>");
    assert_eq!(fx.message.as_deref(), Some("match 1 of 2"));
    let fx = feed(&mut s, "n");
    assert_eq!(fx.message.as_deref(), Some("match 2 of 2"));
    let fx = feed(&mut s, "n"); // wraps
    assert_eq!(fx.message.as_deref(), Some("match 1 of 2"));
    let fx = feed(&mut s, "N");
    assert_eq!(fx.message.as_deref(), Some("match 2 of 2"));
    // `*` reports too; an operator's search motion does not land, so not.
    let mut s = session("one two one");
    let fx = feed(&mut s, "*");
    assert_eq!(fx.message.as_deref(), Some("match 2 of 2"));
    let mut s = session("keep 123 drop");
    let fx = feed(&mut s, r"d/\d<cr>");
    assert_eq!(fx.message, None);
}

#[test]
fn typing_a_search_previews_matches() {
    let mut s = session("foo bar\nbaz bar\nqux");
    // Opening the prompt starts an empty preview session.
    let fx = s.key(Key::Char('/'));
    assert_eq!(fx.search, Some(SearchUi::Active { matches: vec![], current: None }));
    feed(&mut s, "ba");
    let fx = s.key(Key::Char('r'));
    assert_eq!(
        fx.search,
        Some(SearchUi::Active {
            matches: vec![1, 4, 7],
            current: Some([0, 4, 7]),
        })
    );
    // The cursor has not moved: the peek is the host's business.
    assert_eq!(s.cursor(), Pos::new(0, 0));
    // Editing the pattern re-previews; an unmatched pattern previews empty.
    let fx = s.key(Key::Char('z'));
    assert_eq!(fx.search, Some(SearchUi::Active { matches: vec![], current: None }));
    // Enter commits; the search field says to keep the view.
    let fx = s.key(Key::Backspace);
    assert_eq!(fx.search.as_ref(), Some(&SearchUi::Active {
        matches: vec![1, 4, 7],
        current: Some([0, 4, 7]),
    }));
    let fx = s.key(Key::Enter);
    assert_eq!(fx.search, Some(SearchUi::Committed));
    assert_eq!(s.cursor(), Pos::new(0, 4));
}

#[test]
fn abandoning_a_search_says_so() {
    let mut s = session("alpha beta");
    feed(&mut s, "/beta");
    let fx = s.key(Key::Esc);
    assert_eq!(fx.search, Some(SearchUi::Cancelled));
    assert_eq!(s.cursor(), Pos::new(0, 0));
    // Backspacing past the start of the pattern abandons it too.
    feed(&mut s, "/b");
    let fx = s.key(Key::Backspace);
    assert_eq!(fx.search.as_ref(), Some(&SearchUi::Active { matches: vec![], current: None }));
    let fx = s.key(Key::Backspace);
    assert_eq!(fx.search, Some(SearchUi::Cancelled));
    // Enter on an empty prompt with no history closes the session as well.
    let fx = feed(&mut s, "/<cr>");
    assert_eq!(fx.search, Some(SearchUi::Cancelled));
    // An outside cursor move (mouse click) while typing cancels the prompt.
    feed(&mut s, "/beta");
    let fx = s.set_position(0, 3);
    assert_eq!(fx.search, Some(SearchUi::Cancelled));
    let fx = s.set_position(0, 0);
    assert_eq!(fx.search, None);
}

#[test]
fn n_without_a_previous_search_does_nothing() {
    let mut s = session("abc");
    feed(&mut s, "n");
    assert_eq!((s.cursor(), s.text().as_str()), (Pos::new(0, 0), "abc"));
}

#[test]
fn star_and_hash_match_whole_words() {
    let mut s = session("foo foobar\nxfoo foo\nfoo");
    feed(&mut s, "*");
    assert_eq!(s.cursor(), Pos::new(1, 5)); // skips "foobar" and "xfoo"
    feed(&mut s, "*");
    assert_eq!(s.cursor(), Pos::new(2, 0));
    feed(&mut s, "#");
    assert_eq!(s.cursor(), Pos::new(1, 5));
    // `n` after `#` keeps the backward direction.
    feed(&mut s, "n");
    assert_eq!(s.cursor(), Pos::new(0, 0));
}

#[test]
fn star_uses_the_word_the_cursor_sits_in() {
    let mut s = at("alpha beta\nbeta gamma", 0, 8); // inside "beta"
    feed(&mut s, "*");
    assert_eq!(s.cursor(), Pos::new(1, 0));
    // On a non-keyword char, the next keyword on the line is used.
    let mut s = at("a + beta\nbeta", 0, 2);
    feed(&mut s, "*");
    assert_eq!(s.cursor(), Pos::new(1, 0));
    // No keyword on the line at all: nothing happens.
    let mut s = session("  + -");
    feed(&mut s, "*");
    assert_eq!(s.cursor(), Pos::new(0, 0));
}

#[test]
fn search_is_an_exclusive_operator_motion() {
    let mut s = session("foo bar baz");
    feed(&mut s, "d/baz<cr>");
    assert_eq!(s.text(), "baz");
    let mut s = at("foo bar baz", 0, 8);
    feed(&mut s, "d?bar<cr>"); // backward: deletes [match, cursor)
    assert_eq!(s.text(), "foo baz");
    let mut s = session("one two one two");
    feed(&mut s, "*"); // to the second "one"
    assert_eq!(s.cursor(), Pos::new(0, 8));
    feed(&mut s, "0dn"); // operator + repeat
    assert_eq!(s.text(), "one two");
}

#[test]
fn search_extends_a_visual_selection() {
    let mut s = session("hello brave world");
    // Visual mode is inclusive: the match's first char goes too.
    feed(&mut s, "v/world<cr>d");
    assert_eq!(s.text(), "orld");
}

#[test]
fn search_pattern_escapes() {
    // `\<`/`\>` anchor to word boundaries, other escapes are literal.
    let mut s = session("about a bat");
    feed_literal(&mut s, "/\\<a\\>");
    s.key(Key::Enter);
    assert_eq!(s.cursor(), Pos::new(0, 6));
    let mut s = session("a.b axb");
    feed_literal(&mut s, "/a\\.b");
    s.key(Key::Enter);
    assert_eq!(s.cursor(), Pos::new(0, 0)); // literal dot, wrapped to itself
}

#[test]
fn search_patterns_are_regular_expressions() {
    let mut s = session("alpha 42\nbeta 7\ngamma");
    feed(&mut s, r"/\d\+<cr>");
    assert_eq!(s.cursor(), Pos::new(0, 6));
    feed(&mut s, "n"); // matches may start inside the previous one, as vim's do
    assert_eq!(s.cursor(), Pos::new(0, 7));
    feed(&mut s, "n");
    assert_eq!(s.cursor(), Pos::new(1, 5));
    // Anchors, quantifiers, classes, alternation and very magic all work.
    let mut s = session("foo bar\nbar foo");
    feed(&mut s, "/^bar<cr>");
    assert_eq!(s.cursor(), Pos::new(1, 0));
    let mut s = session("ab a1b axb");
    feed(&mut s, "/a[0-9]b<cr>");
    assert_eq!(s.cursor(), Pos::new(0, 3));
    let mut s = session("one two three");
    feed(&mut s, r"/\vt(wo|hree)<cr>");
    assert_eq!(s.cursor(), Pos::new(0, 4));
    // `\c` ignores case for one search.
    let mut s = session("hello HELLO");
    feed(&mut s, r"/\cHELLO<cr>");
    assert_eq!(s.cursor(), Pos::new(0, 6));
    feed(&mut s, "n");
    assert_eq!(s.cursor(), Pos::new(0, 0));
}

#[test]
fn regex_search_composes_with_operators_and_reports_failures() {
    let mut s = session("keep 123 drop");
    feed(&mut s, r"d/\d<cr>");
    assert_eq!(s.text(), "123 drop");
    let fx = feed(&mut s, "/nope<cr>");
    assert_eq!(fx.message.as_deref(), Some("pattern not found: nope"));
    // An unparseable pattern says so instead of moving.
    let fx = feed(&mut s, r"/\(oops<cr>");
    assert_eq!(fx.message.as_deref(), Some("unmatched ( in pattern"));
    assert_eq!(s.cursor(), Pos::new(0, 0));
}

#[test]
fn substitute_on_the_current_line() {
    let mut s = session("foo foo foo\nfoo");
    let fx = feed(&mut s, ":s/foo/bar/<cr>");
    assert_eq!(s.text(), "bar foo foo\nfoo");
    assert_eq!(fx.message.as_deref(), Some("1 substitution on 1 line"));
    assert_eq!(s.cursor(), Pos::new(0, 0));
    let fx = feed(&mut s, ":s/foo/bar/g<cr>");
    assert_eq!(s.text(), "bar bar bar\nfoo");
    assert_eq!(fx.message.as_deref(), Some("2 substitutions on 1 line"));
    // One edit per changed line, in pre-state coordinates.
    let mut s = session("x\ny\nx");
    let fx = feed(&mut s, ":%s/x/zz/<cr>");
    assert_eq!(s.text(), "zz\ny\nzz");
    assert_eq!(fx.edits.len(), 2);
    assert_eq!((fx.edits[1].start, fx.edits[1].end), (Pos::new(2, 0), Pos::new(2, 1)));
    assert_eq!(fx.message.as_deref(), Some("2 substitutions on 2 lines"));
    assert_eq!(s.cursor(), Pos::new(2, 0));
}

#[test]
fn substitute_ranges() {
    let mut s = session("a\na\na\na");
    feed(&mut s, ":2,3s/a/b/<cr>");
    assert_eq!(s.text(), "a\nb\nb\na");
    assert_eq!(s.cursor(), Pos::new(2, 0));
    feed(&mut s, ":%s/a/c/<cr>");
    assert_eq!(s.text(), "c\nb\nb\nc");
    // Relative addresses count from the cursor.
    let mut s = at("a\na\na\na", 1, 0);
    feed(&mut s, ":.,+1s/a/b/<cr>");
    assert_eq!(s.text(), "a\nb\nb\na");
    // `$` is the last line.
    let mut s = session("a\na\na");
    feed(&mut s, ":$s/a/z/<cr>");
    assert_eq!(s.text(), "a\na\nz");
    // A bare address just moves.
    let mut s = session("one\ntwo\n  three");
    feed(&mut s, ":3<cr>");
    assert_eq!(s.cursor(), Pos::new(2, 2));
}

#[test]
fn substitute_over_a_visual_selection() {
    let mut s = session("a\na\na\na");
    // `:` in visual mode prefills the range.
    let fx = feed(&mut s, "Vj:");
    assert_eq!(fx.pending, ":'<,'>");
    feed(&mut s, "s/a/b/<cr>");
    assert_eq!(s.text(), "b\nb\na\na");
    assert_eq!(s.mode_label(), "normal");
}

#[test]
fn substitute_flags() {
    // `i` / `I` override case sensitivity, `n` only counts.
    let mut s = session("Foo foo FOO");
    feed(&mut s, ":s/foo/x/gi<cr>");
    assert_eq!(s.text(), "x x x");
    let mut s = session("Foo foo");
    feed(&mut s, ":s/foo/x/g<cr>");
    assert_eq!(s.text(), "Foo x");
    let mut s = session("a a\na");
    let fx = feed(&mut s, ":%s/a/b/gn<cr>");
    assert_eq!(s.text(), "a a\na"); // unchanged
    assert_eq!(fx.message.as_deref(), Some("3 matches on 2 lines"));
    // A pattern that is not there reports it, unless `e` asks for quiet.
    let fx = feed(&mut s, ":s/zzz/x/<cr>");
    assert_eq!(fx.message.as_deref(), Some("pattern not found: zzz"));
    let fx = feed(&mut s, ":s/zzz/x/e<cr>");
    assert_eq!(fx.message, None);
}

#[test]
fn substitute_replacement_syntax() {
    let mut s = session("size=42");
    feed_literal(&mut s, r":s/\(\w\+\)=\(\d\+\)/\2 is \1/");
    s.key(Key::Enter);
    assert_eq!(s.text(), "42 is size");
    // `&` is the whole match, `\u` upper-cases what follows.
    let mut s = session("one two");
    feed(&mut s, r":s/\w\+/[&]/g<cr>");
    assert_eq!(s.text(), "[one] [two]");
    let mut s = session("foo bar");
    feed(&mut s, r":s/\w\+/\u&/g<cr>");
    assert_eq!(s.text(), "Foo Bar");
    // `\r` breaks the line; the cursor follows to the last new line.
    let mut s = session("a,b\nc,d");
    feed(&mut s, r":%s/,/\r/g<cr>");
    assert_eq!(s.text(), "a\nb\nc\nd");
    assert_eq!(s.cursor(), Pos::new(3, 0));
}

#[test]
fn substitute_reuses_the_last_pattern_and_replacement() {
    let mut s = session("foo\nfoo\nfoo\nfoo");
    feed(&mut s, ":s/foo/bar/<cr>");
    // `&` repeats it on the current line, a bare `:s` does the same.
    feed(&mut s, "j&");
    assert_eq!(s.text(), "bar\nbar\nfoo\nfoo");
    feed(&mut s, ":+1s<cr>");
    assert_eq!(s.text(), "bar\nbar\nbar\nfoo");
    // An empty pattern reuses the last search.
    feed(&mut s, "/foo<cr>");
    feed(&mut s, ":s//baz/<cr>");
    assert_eq!(s.text(), "bar\nbar\nbar\nbaz");
    // `~` in a replacement stands for the previous replacement.
    let mut s = session("x");
    feed(&mut s, ":s/x/ab/<cr>");
    feed(&mut s, ":s/ab/~c/<cr>");
    assert_eq!(s.text(), "abc");
}

#[test]
fn substitute_after_search_and_star() {
    let mut s = session("alpha beta\nalpha");
    feed(&mut s, "*"); // sets the search to \<alpha\>
    feed(&mut s, ":%s//A/<cr>");
    assert_eq!(s.text(), "A beta\nA");
    // `:s` also sets the last search pattern, so `n` follows it.
    let mut s = session("x1\nx2\nx1");
    feed(&mut s, ":s/x1/y/<cr>");
    feed(&mut s, "n");
    assert_eq!(s.cursor(), Pos::new(2, 0));
}

#[test]
fn ex_prompt_is_editable_and_cancelable() {
    let mut s = session("foo");
    let fx = s.key(Key::Char(':'));
    assert_eq!(fx.pending, ":");
    let fx = feed(&mut s, "s/foo/bat");
    assert_eq!(fx.pending, ":s/foo/bat");
    let fx = feed(&mut s, "<bs>r/<cr>");
    assert_eq!((s.text().as_str(), fx.pending.as_str()), ("bar", ""));
    // Escape abandons the line, backspacing past the `:` too.
    feed(&mut s, ":s/bar/zzz/<esc>");
    assert_eq!(s.text(), "bar");
    let fx = feed(&mut s, ":<bs>");
    assert_eq!(fx.pending, "");
    // Digits and operators typed into the line are just text.
    let mut s = session("d3w");
    feed(&mut s, ":s/d3w/ok/<cr>");
    assert_eq!(s.text(), "ok");
}

#[test]
fn ex_errors_are_reported() {
    let mut s = session("abc");
    let fx = feed(&mut s, ":frob 1<cr>");
    assert_eq!(fx.message.as_deref(), Some("E492: Not an editor command: frob 1"));
    let fx = feed(&mut s, ":s/a/b/c<cr>");
    assert_eq!(fx.message.as_deref(), Some("the c (confirm) flag is not supported"));
    let fx = feed(&mut s, ":s/a/b/q<cr>");
    assert_eq!(fx.message.as_deref(), Some("unknown :s flag: q"));
    let fx = feed(&mut s, r":s/\(a/b/<cr>");
    assert_eq!(fx.message.as_deref(), Some("unmatched ( in pattern"));
    assert_eq!(s.text(), "abc"); // nothing was touched
    let mut s = session("abc");
    let fx = feed(&mut s, ":s<cr>");
    assert_eq!(fx.message.as_deref(), Some("no previous regular expression"));
}

#[test]
fn search_keys_are_literal_in_the_prompt() {
    // Digits, operators and specials typed into a pattern are just text.
    let mut s = session("no match\nd3w here");
    feed(&mut s, "/d3w<cr>");
    assert_eq!(s.cursor(), Pos::new(1, 0));
    assert_eq!(s.text(), "no match\nd3w here");
}

// ---- ex line and file commands ---------------------------------------------

/// Run `:{line}<cr>`, and check its edits do to a copy of the document what
/// they did to the mirror when applied the way the host applies them: every
/// one against the text from before the key, in one transaction, none of
/// them overlapping.
fn ex(s: &mut Session, line: &str) -> Effects {
    let before = s.text();
    s.key(Key::Char(':'));
    feed_literal(s, line);
    let fx = s.key(Key::Enter);
    let mut edits: Vec<&Edit> = fx.edits.iter().collect();
    edits.sort_by_key(|e| std::cmp::Reverse((e.start, e.end)));
    for pair in edits.windows(2) {
        assert!(pair[1].end <= pair[0].start, "overlapping edits for :{line}: {edits:?}");
    }
    let mut host = Buffer::from_text(&before);
    for e in edits {
        host.apply_change(e.start, e.end, &e.text);
    }
    assert_eq!(host.text(), s.text(), "the host's copy after :{line}");
    fx
}

#[test]
fn ex_delete_and_yank_use_the_register_dd_and_yy_do() {
    let mut s = at("one\ntwo\nthree\nfour\nfive", 1, 1);
    let fx = ex(&mut s, "d");
    assert_eq!(s.text(), "one\nthree\nfour\nfive");
    assert_eq!((s.cursor(), fx.message), (Pos::new(1, 0), None));
    feed(&mut s, "p");
    assert_eq!(s.text(), "one\nthree\ntwo\nfour\nfive");

    // `:y` leaves the cursor where it is.
    let fx = ex(&mut s, "4,5y");
    assert_eq!((s.cursor(), fx.edits.len()), (Pos::new(2, 0), 0));
    feed(&mut s, "P");
    assert_eq!(s.text(), "one\nthree\nfour\nfive\ntwo\nfour\nfive");
    let fx = ex(&mut s, "%y");
    assert_eq!(fx.message.as_deref(), Some("7 lines yanked"));

    // A count, the black hole, and a named register.
    let mut s = at("a\nb\nc\nd", 0, 0);
    feed(&mut s, "yy");
    ex(&mut s, "2d _ 2");
    assert_eq!(s.text(), "a\nd");
    feed(&mut s, "p");
    assert_eq!(s.text(), "a\nd\na", "the black hole kept the register");
    let fx = ex(&mut s, "d x");
    assert_eq!(fx.message, None);
    assert_eq!(s.text(), "a\nd");
    feed(&mut s, "gg\"xp");
    assert_eq!(s.text(), "a\na\nd");

    // The last lines, and the whole buffer.
    let mut s = at("a\nb\nc", 0, 0);
    ex(&mut s, "2,$d");
    assert_eq!((s.text().as_str(), s.cursor()), ("a", Pos::new(0, 0)));
    let mut s = at("a\nb\nc\nd", 2, 0);
    let fx = ex(&mut s, "%d");
    assert_eq!((s.text().as_str(), fx.message.as_deref()), ("", Some("4 fewer lines")));
}

#[test]
fn ex_move_lines() {
    let mut s = at("a\nb\nc\nd\ne", 0, 0);
    ex(&mut s, "m$");
    assert_eq!((s.text().as_str(), s.cursor()), ("b\nc\nd\ne\na", Pos::new(4, 0)));
    ex(&mut s, "m0");
    assert_eq!((s.text().as_str(), s.cursor()), ("a\nb\nc\nd\ne", Pos::new(0, 0)));
    // Down past other lines, and up from the end.
    ex(&mut s, "2,3m4");
    assert_eq!((s.text().as_str(), s.cursor()), ("a\nd\nb\nc\ne", Pos::new(3, 0)));
    let fx = ex(&mut s, "3,$m0");
    assert_eq!((s.text().as_str(), s.cursor()), ("b\nc\ne\na\nd", Pos::new(2, 0)));
    assert_eq!(fx.message.as_deref(), Some("3 lines moved"));
    // Relative to the cursor, as `:m+1` and `:m-2` are usually typed.
    ex(&mut s, "m+1");
    assert_eq!((s.text().as_str(), s.cursor()), ("b\nc\na\ne\nd", Pos::new(3, 0)));
    ex(&mut s, "m-3");
    assert_eq!((s.text().as_str(), s.cursor()), ("b\ne\nc\na\nd", Pos::new(1, 0)));

    // Into itself is an error; in place is no edit at all.
    let fx = ex(&mut s, "1,3m2");
    assert_eq!(fx.message.as_deref(), Some("E134: Cannot move a range of lines into itself"));
    let fx = ex(&mut s, "2,3m1");
    assert!(fx.edits.is_empty());
    assert_eq!((s.text().as_str(), s.cursor()), ("b\ne\nc\na\nd", Pos::new(2, 0)));

    // The last line up, past an empty line: the removal and the insertion
    // meet at the same position.
    let mut s = at("x\n\ny", 2, 0);
    ex(&mut s, "m1");
    assert_eq!(s.text(), "x\ny\n");
    let mut s = at("  x\n\ny", 0, 0);
    ex(&mut s, "m$");
    assert_eq!((s.text().as_str(), s.cursor()), ("\ny\n  x", Pos::new(2, 2)));
}

#[test]
fn ex_copy_lines() {
    let mut s = at("a\nb", 0, 0);
    ex(&mut s, "t.");
    assert_eq!((s.text().as_str(), s.cursor()), ("a\na\nb", Pos::new(1, 0)));
    ex(&mut s, "1,2t$");
    assert_eq!((s.text().as_str(), s.cursor()), ("a\na\nb\na\na", Pos::new(4, 0)));
    let fx = ex(&mut s, "3,$co0");
    assert_eq!((s.text().as_str(), s.cursor()), ("b\na\na\na\na\nb\na\na", Pos::new(2, 0)));
    assert_eq!(fx.message.as_deref(), Some("3 more lines"));
}

#[test]
fn ex_join() {
    // Vim's rules: leading white space goes, one space goes in — none before
    // `)` — and an empty line adds nothing.
    let mut s = at("  a\n   b\n\n  )c\nd", 0, 0);
    ex(&mut s, "1,4j");
    assert_eq!((s.text().as_str(), s.cursor()), ("  a b)c\nd", Pos::new(0, 2)));
    // One line joins the next; the last line has none to join.
    ex(&mut s, "j");
    assert_eq!(s.text(), "  a b)c d");
    let fx = ex(&mut s, "j");
    assert!(fx.edits.is_empty());
    // No space after a tab or a space; `:j!` keeps the lines as they are.
    let mut s = at("a\t\nb \n c\n  d", 0, 0);
    ex(&mut s, "j 3");
    assert_eq!(s.text(), "a\tb c\n  d");
    ex(&mut s, "j!");
    assert_eq!(s.text(), "a\tb c  d");
    // `:2,2j` names one line, which joins with nothing.
    let mut s = at("a\nb", 0, 0);
    ex(&mut s, "1,1j");
    assert_eq!(s.text(), "a\nb");
}

#[test]
fn ex_shift_asks_the_host_to_indent() {
    let mut s = at("a\nb\nc\nd", 1, 0);
    let fx = ex(&mut s, ">>");
    assert!(matches!(
        fx.commands[..],
        [Command::IndentLines { start_line: 1, end_line: 1, dedent: false, times: 2 }]
    ));
    let fx = ex(&mut s, "%<");
    assert!(matches!(
        fx.commands[..],
        [Command::IndentLines { start_line: 0, end_line: 3, dedent: true, times: 1 }]
    ));
    assert_eq!(fx.message.as_deref(), Some("4 lines <ed 1 time"));
    // From visual mode, over the selection.
    let mut s = at("a\nb\nc\nd", 1, 0);
    feed(&mut s, "Vj");
    let fx = ex(&mut s, ">");
    assert!(matches!(
        fx.commands[..],
        [Command::IndentLines { start_line: 1, end_line: 2, dedent: false, times: 1 }]
    ));
    assert_eq!(fx.mode, "normal");
}

#[test]
fn ex_sort() {
    let mut s = at("c\na\nB\nb", 2, 0);
    ex(&mut s, "sort");
    assert_eq!((s.text().as_str(), s.cursor()), ("B\na\nb\nc", Pos::new(0, 0)));
    ex(&mut s, "sort!");
    assert_eq!(s.text(), "c\nb\na\nB");
    // Case folded, and stable: `a` and `A` keep the order they came in.
    let mut s = session("c\na\nB\nA\nb");
    ex(&mut s, "sort i");
    assert_eq!(s.text(), "a\nA\nB\nb\nc");

    // Numbers, with a line without one first.
    let mut s = session("x10\nx-2\nnone\nx3");
    ex(&mut s, "sort n");
    assert_eq!(s.text(), "none\nx-2\nx3\nx10");

    // Unique compares whole lines, case folded with `i`.
    let mut s = session("b\na\nb\nA");
    ex(&mut s, "sort u");
    assert_eq!(s.text(), "A\na\nb");
    let mut s = session("b\na\nb\nA\nc\nC");
    let fx = ex(&mut s, "sort ui");
    assert_eq!((s.text().as_str(), fx.message.as_deref()), ("a\nb\nc", Some("3 fewer lines")));

    // On what follows a pattern, or (with `r`) on what it matched.
    let mut s = session("a3 z\nb1 y\nc2 x");
    ex(&mut s, r"sort /\a\d /");
    assert_eq!(s.text(), "c2 x\nb1 y\na3 z");
    ex(&mut s, r"sort n /\d/ r");
    assert_eq!(s.text(), "b1 y\nc2 x\na3 z");

    // A range; an already sorted one is no edit.
    let mut s = session("z\nb\na\ny");
    ex(&mut s, "2,3sort");
    assert_eq!(s.text(), "z\na\nb\ny");
    let fx = ex(&mut s, "2,3sort");
    assert!(fx.edits.is_empty());
    let fx = ex(&mut s, "sort //");
    assert_eq!(fx.message.as_deref(), Some("E35: No previous regular expression"));
}

#[test]
fn ex_global_runs_a_command_on_every_matching_line() {
    let mut s = session("a1\nb\na2\nb\na3");
    let fx = ex(&mut s, "g/a/d");
    assert_eq!((s.text().as_str(), fx.message.as_deref()), ("b\nb", Some("3 fewer lines")));
    assert_eq!(fx.edits.len(), 1, "one edit, one undo step");

    let mut s = session("a1\nb\na2\nb\na3");
    ex(&mut s, "v/a/d");
    assert_eq!(s.text(), "a1\na2\na3");

    // The classic reversal: each match moves to the top in turn.
    let mut s = session("a1\nb\na2\nb\na3");
    ex(&mut s, "g/a/m0");
    assert_eq!(s.text(), "a3\na2\na1\nb\nb");
    // ...and to the bottom: the marks travel past the line being moved.
    let mut s = session("x1\ny\nx2\nz");
    ex(&mut s, "g/x/m$");
    assert_eq!(s.text(), "y\nz\nx1\nx2");
    // Copies are not visited again.
    let mut s = session("a\nb\na");
    ex(&mut s, "g/a/t$");
    assert_eq!(s.text(), "a\nb\na\na\na");

    // Blank lines, however many adjoin, wherever they are.
    let mut s = session("\nx\n\n\ny\n\n");
    ex(&mut s, "g/^$/d");
    assert_eq!(s.text(), "x\ny");

    // A join swallows the line below, marked or not.
    let mut s = session("a\nb\na\nc");
    ex(&mut s, "g/a/j");
    assert_eq!(s.text(), "a b\na c");
    let mut s = session("a\na\na\nb");
    ex(&mut s, "g/a/j");
    assert_eq!(s.text(), "a a\na b");

    // A range limits the lines looked at.
    let mut s = session("a\na\na\na");
    ex(&mut s, "2,3g/a/s/a/b/");
    assert_eq!(s.text(), "a\nb\nb\na");
}

#[test]
fn ex_global_reports_once_and_stops_at_errors() {
    let mut s = session("a1\nb\na22");
    let fx = ex(&mut s, r"g/a/s/\d/N/g");
    assert_eq!(s.text(), "aN\nb\naNN");
    assert_eq!(fx.message.as_deref(), Some("3 substitutions on 2 lines"));
    assert_eq!(s.cursor(), Pos::new(2, 0));
    // A line `:s` finds nothing on is no error.
    let mut s = session("a1\na\na2");
    let fx = ex(&mut s, r"g/a/s/\d/N/");
    assert_eq!((s.text().as_str(), fx.message.as_deref()), ("aN\na\naN", Some("2 substitutions on 2 lines")));

    let mut s = session("a\nb");
    let fx = ex(&mut s, "g/zzz/d");
    assert_eq!((s.text().as_str(), fx.message.as_deref()), ("a\nb", Some("Pattern not found: zzz")));
    let fx = ex(&mut s, "v/./d");
    assert_eq!(fx.message.as_deref(), Some("Pattern found in every line: ."));
    let fx = ex(&mut s, "g/a/");
    assert_eq!(fx.message.as_deref(), Some("1 matching line"));
    let fx = ex(&mut s, "g/./frob");
    assert_eq!(fx.message.as_deref(), Some("E492: Not an editor command: frob"));
    let fx = ex(&mut s, "g/./w");
    assert_eq!(fx.message.as_deref(), Some(":global cannot run file commands"));
    assert!(fx.commands.is_empty());
    let fx = ex(&mut s, "g/a/1g/b/d");
    assert_eq!(fx.message.as_deref(), Some("E147: Cannot do :global recursive with a range"));
    assert_eq!(s.text(), "a\nb");

    // An error part way leaves what was done done, and says so: the second
    // `a` has no line two below it to move under.
    let mut s = session("a\nb\na");
    let fx = ex(&mut s, "g/a/m+2");
    assert_eq!(s.text(), "b\na\na");
    assert_eq!(fx.message.as_deref(), Some("E16: Invalid range"));
}

#[test]
fn ex_global_nests_and_sets_the_search() {
    // `:g/a/v/1/d`: lines with an a and no 1.
    let mut s = session("a1\na2\nb1\na3");
    ex(&mut s, "g/a/v/1/d");
    assert_eq!(s.text(), "a1\nb1");
    // The pattern becomes the last search, for `n` and `:s//`.
    let mut s = session("x\nfoo\nx\nfoo");
    ex(&mut s, "g/foo/s//bar/");
    assert_eq!(s.text(), "x\nbar\nx\nbar");
    // Shifts are the host's, one per line, all in one go.
    let mut s = session("x\nfoo\nx\nfoo");
    let fx = ex(&mut s, "g/foo/>");
    assert!(matches!(
        fx.commands[..],
        [
            Command::IndentLines { start_line: 1, end_line: 1, .. },
            Command::IndentLines { start_line: 3, end_line: 3, .. },
        ]
    ));
    feed(&mut s, "ggn");
    assert_eq!(s.cursor(), Pos::new(1, 0));
}

#[test]
fn zz_and_zq_close_the_editor_through_the_host() {
    let json = |fx: &Effects| serde_json::to_string(&fx.commands).expect("serializes");
    let mut s = session("a");
    let fx = feed(&mut s, "ZZ");
    assert_eq!(json(&fx), r#"[{"kind":"file","write":"ifModified","close":true,"all":false,"force":false}]"#);
    let fx = feed(&mut s, "ZQ");
    assert_eq!(json(&fx), r#"[{"kind":"file","write":"never","close":true,"all":false,"force":true}]"#);
    assert_eq!(fx.pending, "");
    // Anything else after `Z` drops it.
    let fx = feed(&mut s, "Zj");
    assert!(fx.commands.is_empty());
    assert_eq!((fx.pending.as_str(), s.text().as_str()), ("", "a"));
    let fx = feed(&mut s, "Z");
    assert_eq!(fx.pending, "Z");
}

#[test]
fn ex_file_commands_go_to_the_host() {
    let json = |fx: &Effects| serde_json::to_string(&fx.commands).expect("serializes");
    let mut s = session("a\nb");
    let fx = ex(&mut s, "w");
    assert_eq!(json(&fx), r#"[{"kind":"file","write":"always","close":false,"all":false,"force":false}]"#);
    let fx = ex(&mut s, "wqa");
    assert_eq!(json(&fx), r#"[{"kind":"file","write":"ifModified","close":true,"all":true,"force":false}]"#);
    let fx = ex(&mut s, "e!");
    assert_eq!(json(&fx), r#"[{"kind":"reload","force":true}]"#);
    let fx = ex(&mut s, "noh");
    assert!(fx.commands.is_empty() && fx.message.is_none());
    let fx = ex(&mut s, "w foo.txt");
    assert!(fx.commands.is_empty());
    assert_eq!(fx.message.as_deref(), Some("file names are not supported: foo.txt"));
    // From visual mode, `:'<,'>w` would write part of the buffer.
    feed(&mut s, "V");
    let fx = ex(&mut s, "w");
    assert!(fx.commands.is_empty());
    assert_eq!(fx.message.as_deref(), Some("writing part of a buffer is not supported"));
}

#[test]
fn multi_cursor_ex_and_file_commands_run_once() {
    let mut s = session("a\nb\nc\nd");
    cursors(&mut s, &[(0, 0), (2, 0)]);
    let fx = feed(&mut s, "ZZ");
    assert!(matches!(fx.commands[..], [Command::File(_)]));
    let fx = feed(&mut s, ":w<cr>");
    assert!(matches!(fx.commands[..], [Command::File(_)]));
    let fx = feed(&mut s, ":><cr>");
    assert!(matches!(fx.commands[..], [Command::IndentLines { start_line: 0, end_line: 0, .. }]));
    // `:d` on the primary's line; the other cursor rides along.
    let mut s = session("a\nb\nc\nd");
    cursors(&mut s, &[(0, 0), (2, 0)]);
    let fx = feed(&mut s, ":d<cr>");
    assert_eq!(s.text(), "b\nc\nd");
    assert_eq!(actives(&fx), [(0, 0), (1, 0)]);
}


// ---- multi-cursor ----------------------------------------------------------

/// Mirror a multi-cursor selection change from the host: bare cursors, in
/// VSCode order with the primary first.
fn cursors(s: &mut Session, at: &[(usize, usize)]) {
    let sels: Vec<(Pos, Pos)> = at
        .iter()
        .map(|&(line, col)| (Pos::new(line, col), Pos::new(line, col)))
        .collect();
    s.set_cursors(&sels, true);
}

/// Where every cursor ended up.
fn actives(fx: &Effects) -> Vec<(usize, usize)> {
    fx.selections
        .iter()
        .map(|s| (s.active.line, s.active.col))
        .collect()
}

#[test]
fn multi_cursor_shift_i_inserts_at_every_line_start() {
    // cmd+alt+down twice, then I: insert at each line's first non-blank.
    let mut s = at("  one\nfour\n    six", 0, 2);
    cursors(&mut s, &[(0, 2), (1, 2), (2, 2)]);
    let fx = feed(&mut s, "I");
    assert_eq!(fx.mode, "insert");
    assert_eq!(actives(&fx), [(0, 2), (1, 0), (2, 4)]);

    // The host types at all three natively; the engine only mirrors it.
    for (line, col) in [(2, 4), (1, 0), (0, 2)] {
        s.apply_change(Pos::new(line, col), Pos::new(line, col), "X");
    }
    cursors(&mut s, &[(0, 3), (1, 1), (2, 5)]);
    assert_eq!(s.text(), "  Xone\nXfour\n    Xsix");

    // Escape leaves insert mode with the cursors intact; escape again drops
    // back to one cursor.
    let fx = feed(&mut s, "<esc>");
    assert_eq!(fx.mode, "normal");
    assert_eq!(actives(&fx), [(0, 2), (1, 0), (2, 4)]);
    let fx = feed(&mut s, "<esc>");
    assert_eq!(actives(&fx), [(0, 2)]);
}

#[test]
fn multi_cursor_edits_stay_in_pre_state_coordinates() {
    let mut s = session("abc\nabc\nabc");
    cursors(&mut s, &[(0, 1), (1, 1), (2, 1)]);
    let fx = feed(&mut s, "x");
    assert_eq!(s.text(), "ac\nac\nac");
    assert_eq!(actives(&fx), [(0, 1), (1, 1), (2, 1)]);
    // One edit per cursor, each against the document the host still holds.
    let ranges: Vec<_> = fx
        .edits
        .iter()
        .map(|e| (e.start.line, e.start.col, e.end.line, e.end.col))
        .collect();
    assert_eq!(ranges, [(2, 1, 2, 2), (1, 1, 1, 2), (0, 1, 0, 2)]);
}

#[test]
fn multi_cursor_edits_on_one_line_shift_the_ones_after_them() {
    let mut s = session("abcdef");
    cursors(&mut s, &[(0, 1), (0, 3)]);
    let fx = feed(&mut s, "x");
    assert_eq!(s.text(), "acef");
    // The second cursor sat on 'd'; after both deletions it sits on 'e'.
    assert_eq!(actives(&fx), [(0, 1), (0, 2)]);
}

#[test]
fn multi_cursor_o_is_one_command_for_every_cursor() {
    let mut s = session("a\nb\nc");
    cursors(&mut s, &[(0, 0), (1, 0), (2, 0)]);
    let fx = feed(&mut s, "o");
    // The host's line-insert opens a line under each of its cursors on its
    // own, so the engine asks once and lets the changes mirror back.
    assert!(matches!(fx.commands[..], [Command::OpenLine { above: false }]));
    assert!(fx.edits.is_empty());
    assert_eq!(fx.mode, "insert");
    assert_eq!(actives(&fx), [(0, 0), (1, 0), (2, 0)]);
}

#[test]
fn multi_cursor_operators_and_the_shared_register() {
    let mut s = session("foo one\nfoo two");
    cursors(&mut s, &[(0, 0), (1, 0)]);
    feed(&mut s, "dw");
    assert_eq!(s.text(), "one\ntwo");
    // A yank at two cursors fills the register with a piece per cursor, and
    // a put at as many cursors gives each its own piece back.
    let mut s = session("ab\ncd");
    cursors(&mut s, &[(0, 0), (1, 0)]);
    feed(&mut s, "ylp");
    assert_eq!(s.text(), "aab\nccd");
}

#[test]
fn multi_cursor_linewise_delete() {
    let mut s = session("one\ntwo\nthree\nfour");
    cursors(&mut s, &[(0, 0), (2, 0)]);
    let fx = feed(&mut s, "dd");
    assert_eq!(s.text(), "two\nfour");
    assert_eq!(actives(&fx), [(0, 0), (1, 0)]);
}

#[test]
fn multi_cursor_selections_are_visual_mode() {
    // cmd+d style: two selections with a body.
    let mut s = session("foo\nfoo");
    s.set_cursors(
        &[
            (Pos::new(0, 0), Pos::new(0, 3)),
            (Pos::new(1, 0), Pos::new(1, 3)),
        ],
        false,
    );
    assert_eq!(s.mode_label(), "visual");
    feed(&mut s, "d");
    assert_eq!(s.text(), "\n");
    assert_eq!(s.mode_label(), "normal");
}

#[test]
fn multi_cursor_runs_document_wide_things_once() {
    let mut s = session("a\nb");
    cursors(&mut s, &[(0, 0), (1, 0)]);
    let fx = feed(&mut s, "u");
    assert!(matches!(fx.commands[..], [Command::Undo]));
    // An ex command already spans a range; running it per cursor would
    // double every substitution.
    let mut s = session("aa\naa");
    cursors(&mut s, &[(0, 0), (1, 0)]);
    let fx = feed(&mut s, ":%s/a/b/g<cr>");
    assert_eq!(s.text(), "bb\nbb");
    assert_eq!(fx.message.as_deref(), Some("4 substitutions on 2 lines"));
}

#[test]
fn multi_cursor_ex_edits_carry_the_cursors_that_sat_it_out() {
    // The primary is below the cursor the substitution moves, and the
    // replacement breaks a line: the other cursor still has to follow.
    let mut s = session("a\nb\nc\nd");
    cursors(&mut s, &[(3, 0), (2, 0)]);
    let fx = feed(&mut s, ":1s/a/x\\rY/<cr>");
    assert_eq!(s.text(), "x\nY\nb\nc\nd");
    assert_eq!(actives(&fx), [(1, 0), (3, 0)]);
}

#[test]
fn multi_cursor_indent_reaches_every_line() {
    let mut s = session("a\nb\nc");
    cursors(&mut s, &[(0, 0), (2, 0)]);
    let fx = feed(&mut s, ">>");
    assert!(matches!(
        fx.commands[..],
        [
            Command::IndentLines { start_line: 2, end_line: 2, dedent: false, times: 1 },
            Command::IndentLines { start_line: 0, end_line: 0, dedent: false, times: 1 },
        ]
    ));
}

#[test]
fn multi_cursor_search_moves_every_cursor() {
    let mut s = session("x foo\nx foo\nx foo");
    cursors(&mut s, &[(0, 0), (1, 0)]);
    let fx = feed(&mut s, "/foo<cr>");
    assert_eq!(actives(&fx), [(0, 2), (1, 2)]);
    // The primary's report is the one the status bar gets.
    assert_eq!(fx.message.as_deref(), Some("match 1 of 3"));
}

#[test]
fn multi_cursor_collapses_when_two_cursors_meet() {
    let mut s = session("abc");
    cursors(&mut s, &[(0, 0), (0, 1)]);
    let fx = feed(&mut s, "$");
    assert_eq!(actives(&fx), [(0, 2)]);
}

// ---- r<cr> -----------------------------------------------------------------

#[test]
fn r_with_enter_breaks_the_line() {
    let mut s = at("abcdef", 0, 2);
    feed(&mut s, "r<cr>");
    assert_eq!(s.text(), "ab\ndef");
    assert_eq!(s.cursor(), Pos::new(1, 0));
    // A count replaces that many chars with a single break, not with count
    // of them.
    let mut s = at("abcdef", 0, 1);
    feed(&mut s, "3r<cr>");
    assert_eq!(s.text(), "a\nef");
    assert_eq!(s.cursor(), Pos::new(1, 0));
    // On the last char of a line: the break lands at its end.
    let mut s = at("ab", 0, 1);
    feed(&mut s, "r<cr>");
    assert_eq!(s.text(), "a\n");
    assert_eq!(s.cursor(), Pos::new(1, 0));
    // Not enough characters left for the count: r does nothing, as always.
    let mut s = at("ab", 0, 1);
    feed(&mut s, "3r<cr>");
    assert_eq!(s.text(), "ab");
}

#[test]
fn r_with_enter_cancels_in_visual_mode() {
    let mut s = session("hello");
    let fx = feed(&mut s, "vllr<cr>");
    assert_eq!(s.text(), "hello");
    assert_eq!(fx.pending, "");
    assert_eq!(s.mode_label(), "visual");
}

#[test]
fn multi_cursor_r_with_enter_splits_every_line() {
    let mut s = session("a-b\na-b");
    cursors(&mut s, &[(0, 1), (1, 1)]);
    let fx = feed(&mut s, "r<cr>");
    assert_eq!(s.text(), "a\nb\na\nb");
    assert_eq!(actives(&fx), [(1, 0), (3, 0)]);
}

// ---- line, column and screen motions ---------------------------------------
// Expected positions were checked against Vim 9.1 (`vim -u NONE`).

#[test]
fn underscore_goes_to_the_first_non_blank_lines_down() {
    let mut s = at("  one\n\ttwo\nthree", 0, 4);
    feed(&mut s, "_");
    assert_eq!(s.cursor(), Pos::new(0, 2));
    feed(&mut s, "2_");
    assert_eq!(s.cursor(), Pos::new(1, 1));
    feed(&mut s, "9_"); // past the end: the last line
    assert_eq!(s.cursor(), Pos::new(2, 0));
    // Linewise, so `d_` is `dd` and a count reaches further down.
    let mut s = at("a\nb\nc\nd", 1, 0);
    feed(&mut s, "d2_");
    assert_eq!(s.text(), "a\nd");
    // In visual mode it extends the selection.
    let mut s = at("a\n  b\nc", 0, 0);
    let fx = feed(&mut s, "V2_d");
    assert_eq!((fx.mode, s.text().as_str()), ("normal", "c"));
}

#[test]
fn g_underscore_goes_to_the_last_non_blank() {
    let mut s = at("abc   \nx y  \n", 0, 0);
    feed(&mut s, "g_");
    assert_eq!(s.cursor(), Pos::new(0, 2));
    feed(&mut s, "2g_");
    assert_eq!(s.cursor(), Pos::new(1, 2));
    // Inclusive: the last non-blank goes too, the trailing blanks stay.
    let mut s = at("abc   \nx", 0, 0);
    feed(&mut s, "dg_");
    assert_eq!(s.text(), "   \nx");
}

#[test]
fn ge_goes_back_to_word_ends() {
    let mut s = at("abc def+ghi\n\n  jkl", 0, 9);
    feed(&mut s, "ge");
    assert_eq!(s.cursor(), Pos::new(0, 7));
    feed(&mut s, "ge");
    assert_eq!(s.cursor(), Pos::new(0, 6));
    feed(&mut s, "gE");
    assert_eq!(s.cursor(), Pos::new(0, 2));
    // An empty line is a word end of its own.
    let mut s = at("abc\n\n  def", 2, 3);
    feed(&mut s, "gE");
    assert_eq!(s.cursor(), Pos::new(1, 0));
    feed(&mut s, "ge");
    assert_eq!(s.cursor(), Pos::new(0, 2));
}

#[test]
fn ge_with_an_operator_is_inclusive_both_ends() {
    let mut s = at("abc def ghi", 0, 5);
    feed(&mut s, "dge");
    assert_eq!(s.text(), "abf ghi");
    assert_eq!(s.cursor(), Pos::new(0, 2));
    // Out of words before the count is spent: the operator is dropped.
    let mut s = at("a b", 0, 2);
    let fx = feed(&mut s, "d2ge");
    assert_eq!((s.text().as_str(), fx.pending.as_str()), ("a b", ""));
}

#[test]
fn bar_goes_to_a_column_and_keeps_wanting_it() {
    let mut s = at("abcdefgh\nab\nabcdefgh", 0, 7);
    feed(&mut s, "|");
    assert_eq!(s.cursor(), Pos::new(0, 0));
    feed(&mut s, "6|");
    assert_eq!(s.cursor(), Pos::new(0, 5));
    feed(&mut s, "j");
    assert_eq!(s.cursor(), Pos::new(1, 1));
    feed(&mut s, "j"); // column 6 again, not the short line's 2
    assert_eq!(s.cursor(), Pos::new(2, 5));
    // Exclusive, and no further than the last char.
    let mut s = at("abcdefg", 0, 6);
    feed(&mut s, "d3|");
    assert_eq!(s.text(), "abg");
    let mut s = at("abcde", 0, 0);
    feed(&mut s, "d10|");
    assert_eq!(s.text(), "e");
}

#[test]
fn h_m_l_go_to_the_lines_on_screen() {
    let text: String = (0..40).map(|i| format!("  line {i}\n")).collect();
    let mut s = at(&text, 15, 0);
    s.set_view(10, 20);
    feed(&mut s, "H");
    assert_eq!(s.cursor(), Pos::new(10, 2));
    feed(&mut s, "L");
    assert_eq!(s.cursor(), Pos::new(20, 2));
    feed(&mut s, "M");
    assert_eq!(s.cursor(), Pos::new(15, 2));
    feed(&mut s, "3H");
    assert_eq!(s.cursor(), Pos::new(12, 2));
    feed(&mut s, "3L");
    assert_eq!(s.cursor(), Pos::new(18, 2));
    // A count is not held to the screen, only to the buffer.
    feed(&mut s, "30L");
    assert_eq!(s.cursor(), Pos::new(0, 2));
    // Linewise with an operator: from here to the bottom of the screen.
    let mut s = at(&text, 18, 0);
    s.set_view(10, 20);
    feed(&mut s, "dL");
    assert_eq!(s.text().lines().nth(18), Some("  line 21"));
    // No view reported yet: the whole buffer is the screen.
    let mut s = at("a\nb\nc\nd\ne", 0, 0);
    feed(&mut s, "L");
    assert_eq!(s.cursor(), Pos::new(4, 0));
    feed(&mut s, "M");
    assert_eq!(s.cursor(), Pos::new(2, 0));
}

// ---- case operators ----------------------------------------------------------

#[test]
fn case_operators_take_motions_and_text_objects() {
    let mut s = at("abc def ghi", 0, 5);
    let fx = feed(&mut s, "gU");
    assert_eq!(fx.pending, "gU");
    feed(&mut s, "iw");
    assert_eq!(s.text(), "abc DEF ghi");
    assert_eq!(s.cursor(), Pos::new(0, 4));
    feed(&mut s, "0g~$");
    assert_eq!(s.text(), "ABC def GHI");
    assert_eq!(s.cursor(), Pos::new(0, 0));
    feed(&mut s, "gu2w");
    assert_eq!(s.text(), "abc def GHI");
    // Across lines: `gUj` is linewise; the cursor stays put going down and
    // follows the motion going up.
    let mut s = at("abc def\nghi jkl\nmno", 0, 4);
    feed(&mut s, "gUj");
    assert_eq!(s.text(), "ABC DEF\nGHI JKL\nmno");
    assert_eq!(s.cursor(), Pos::new(0, 4));
    feed(&mut s, "jjlgUk");
    assert_eq!(s.text(), "ABC DEF\nGHI JKL\nMNO");
    assert_eq!(s.cursor(), Pos::new(1, 2));
    // `u` alone is still undo.
    let fx = feed(&mut s, "u");
    assert!(matches!(fx.commands[..], [Command::Undo]));
}

#[test]
fn doubled_case_operators_work_on_lines() {
    let mut s = at("   abc def\nghi", 0, 7);
    feed(&mut s, "gUU");
    assert_eq!(s.text(), "   ABC DEF\nghi");
    assert_eq!(s.cursor(), Pos::new(0, 3)); // the first non-blank
    feed(&mut s, "gugu");
    assert_eq!(s.text(), "   abc def\nghi");
    // In the indent it stays where it is.
    let mut s = at("   abc def\nghi", 0, 1);
    feed(&mut s, "g~~");
    assert_eq!(s.text(), "   ABC DEF\nghi");
    assert_eq!(s.cursor(), Pos::new(0, 1));
    // With a count the cursor keeps its column.
    let mut s = at("abc def\nghi\njkl", 0, 4);
    feed(&mut s, "2g~g~");
    assert_eq!(s.text(), "ABC DEF\nGHI\njkl");
    assert_eq!(s.cursor(), Pos::new(0, 4));
    feed(&mut s, "3guu");
    assert_eq!(s.text(), "abc def\nghi\njkl");
}

#[test]
fn visual_u_and_shift_u_change_case() {
    let mut s = at("abc dEf\nghi", 0, 1);
    let fx = feed(&mut s, "vjU");
    assert_eq!(s.text(), "aBC DEF\nGHi");
    assert_eq!((fx.mode, s.cursor()), ("normal", Pos::new(0, 1)));
    let fx = feed(&mut s, "Vu");
    assert_eq!(s.text(), "abc def\nGHi");
    assert_eq!((fx.mode, s.cursor()), ("normal", Pos::new(0, 0)));
    // The `g` spellings do the same in visual mode.
    feed(&mut s, "jvlgU");
    assert_eq!(s.text(), "abc def\nGHi");
    feed(&mut s, "vlg~");
    assert_eq!(s.text(), "abc def\nghi");
}

// ---- actions ---------------------------------------------------------------

#[test]
fn g_shift_j_joins_without_touching_spaces() {
    let mut s = at("abc\n  def\nghi", 0, 0);
    feed(&mut s, "gJ");
    assert_eq!(s.text(), "abc  def\nghi");
    assert_eq!(s.cursor(), Pos::new(0, 3));
    let mut s = at("abc\n  def\nghi", 0, 0);
    feed(&mut s, "3gJ");
    assert_eq!(s.text(), "abc  defghi");
    assert_eq!(s.cursor(), Pos::new(0, 8));
    let mut s = at("abc\n\nghi", 0, 0);
    feed(&mut s, "3gJ");
    assert_eq!((s.text().as_str(), s.cursor()), ("abcghi", Pos::new(0, 3)));
    let mut s = at("abc\n  def\nghi\njkl", 0, 0);
    let fx = feed(&mut s, "VjjgJ");
    assert_eq!((fx.mode, s.text().as_str()), ("normal", "abc  defghi\njkl"));
}

#[test]
fn gv_reselects_the_last_visual_area() {
    let mut s = at("one two\nthree four\nfive", 0, 1);
    let fx = feed(&mut s, "vjl<esc>");
    assert_eq!(fx.mode, "normal");
    feed(&mut s, "G");
    let fx = feed(&mut s, "gv");
    assert_eq!(fx.mode, "visual");
    assert_eq!((fx.selections[0].anchor, s.cursor()), (Pos::new(0, 1), Pos::new(1, 2)));
    // From visual mode, `gv` trades the current area for the last one.
    feed(&mut s, "<esc>GVk");
    let fx = feed(&mut s, "gv");
    assert_eq!((fx.mode, s.cursor()), ("visual", Pos::new(1, 2)));
    let fx = feed(&mut s, "gv");
    assert_eq!((fx.mode, s.cursor()), ("visualLine", Pos::new(1, 0)));
    // An operator ends visual mode too, and the area survives it.
    let mut s = at("abc\ndef", 0, 0);
    feed(&mut s, "Vy");
    let fx = feed(&mut s, "jgv");
    assert_eq!((fx.mode, s.cursor()), ("visualLine", Pos::new(0, 0)));
    // Nothing to reselect yet: nothing happens.
    let mut s = at("abc", 0, 1);
    let fx = feed(&mut s, "gv");
    assert_eq!((fx.mode, s.cursor()), ("normal", Pos::new(0, 1)));
}

#[test]
fn gv_remembers_a_selection_the_pointer_made() {
    let mut s = at("abc def", 0, 0);
    s.set_selection(Pos::new(0, 4), Pos::new(0, 7), true);
    s.set_position(0, 1); // a click ends it
    let fx = feed(&mut s, "gv");
    assert_eq!(fx.mode, "visual");
    assert_eq!((fx.selections[0].anchor, s.cursor()), (Pos::new(0, 4), Pos::new(0, 6)));
}

#[test]
fn gi_and_g_shift_i_enter_insert() {
    let mut s = at("  abc\ndef", 0, 3);
    feed(&mut s, "a");
    type_ins(&mut s, "XY");
    feed(&mut s, "<esc>j0");
    let fx = feed(&mut s, "gi");
    // Where typing stopped, not where the cursor stepped back to.
    assert_eq!((fx.mode, s.cursor()), ("insert", Pos::new(0, 6)));
    feed(&mut s, "<esc>");
    let fx = feed(&mut s, "gI");
    assert_eq!((fx.mode, s.cursor()), ("insert", Pos::new(0, 0)));
    // No insert yet: `gi` is `i`.
    let mut s = at("abc", 0, 2);
    let fx = feed(&mut s, "gi");
    assert_eq!((fx.mode, s.cursor()), ("insert", Pos::new(0, 2)));
}

#[test]
fn gp_leaves_the_cursor_after_the_paste() {
    let mut s = at("abc\ndef", 0, 0);
    feed(&mut s, "lylgp");
    assert_eq!(s.text(), "abbc\ndef");
    assert_eq!(s.cursor(), Pos::new(0, 3));
    feed(&mut s, "$gp"); // past the end of the line: on its last char
    assert_eq!(s.text(), "abbcb\ndef");
    assert_eq!(s.cursor(), Pos::new(0, 4));
    // Lines: the line after the paste, or the last one if there is none.
    let mut s = at("abc\ndef", 0, 0);
    feed(&mut s, "yygp");
    assert_eq!((s.text().as_str(), s.cursor()), ("abc\nabc\ndef", Pos::new(2, 0)));
    feed(&mut s, "gp");
    assert_eq!((s.text().as_str(), s.cursor()), ("abc\nabc\ndef\nabc", Pos::new(3, 0)));
    let mut s = at("abc\ndef", 0, 1);
    feed(&mut s, "yy2gP");
    assert_eq!((s.text().as_str(), s.cursor()), ("abc\nabc\nabc\ndef", Pos::new(2, 0)));
    // Over a visual selection.
    let mut s = at("abc\ndef", 0, 0);
    let fx = feed(&mut s, "yiwjviwgp");
    assert_eq!((fx.mode, s.text().as_str(), s.cursor()), ("normal", "abc\nabc", Pos::new(1, 2)));
}

#[test]
fn ctrl_a_and_ctrl_x_change_the_number() {
    let mut s = at("x = 41; y = -3", 0, 0);
    feed(&mut s, "<c-a>");
    assert_eq!((s.text().as_str(), s.cursor()), ("x = 42; y = -3", Pos::new(0, 5)));
    feed(&mut s, "w5<c-a>"); // from `;`: the next number, sign and all
    assert_eq!((s.text().as_str(), s.cursor()), ("x = 42; y = 2", Pos::new(0, 12)));
    feed(&mut s, "10<c-x>");
    assert_eq!(s.text(), "x = 42; y = -8");
    let mut s = at("mask 0x0f", 0, 0);
    feed(&mut s, "<c-a>");
    assert_eq!((s.text().as_str(), s.cursor()), ("mask 0x10", Pos::new(0, 8)));
    // No number: nothing changes, and nothing is left pending.
    let mut s = at("none here", 0, 0);
    let fx = feed(&mut s, "<c-a>");
    assert!(fx.edits.is_empty());
    assert_eq!(fx.pending, "");
}

#[test]
fn visual_ctrl_a_changes_every_line() {
    let mut s = at("x 1\nx 1\nx 1", 0, 0);
    let fx = feed(&mut s, "VG<c-a>");
    assert_eq!(s.text(), "x 2\nx 2\nx 2");
    assert_eq!((fx.mode, s.cursor()), ("normal", Pos::new(0, 0)));
    // `g ctrl-a` counts up line by line.
    let mut s = at("x 1\nx 1\nx 1", 0, 0);
    feed(&mut s, "VGg<c-a>");
    assert_eq!(s.text(), "x 2\nx 3\nx 4");
    let mut s = at("x 0\nx 0\nx 0", 2, 0);
    feed(&mut s, "Vgg2g<c-x>");
    assert_eq!(s.text(), "x -2\nx -4\nx -6");
    // Charwise, only what the selection covers counts.
    let mut s = at("a 5 b 6\nc -7 d", 0, 2);
    let fx = feed(&mut s, "vj<c-a>");
    assert_eq!(s.text(), "a 6 b 6\nc -7 d");
    assert_eq!((fx.mode, s.cursor()), ("normal", Pos::new(0, 2)));
    // Outside visual mode `g ctrl-a` is nothing.
    let mut s = at("x 1", 0, 0);
    let fx = feed(&mut s, "g<c-a>");
    assert!(fx.edits.is_empty());
}

#[test]
fn ctrl_e_and_ctrl_y_scroll_the_view() {
    let mut s = at("a\nb\nc", 1, 0);
    let fx = feed(&mut s, "3<c-e>");
    assert!(matches!(
        fx.commands[..],
        [Command::ScrollLines { down: true, lines: 3, move_cursor: true }]
    ));
    assert_eq!(s.cursor(), Pos::new(1, 0));
    // In visual mode the selection must survive the scroll.
    let fx = feed(&mut s, "v<c-y>");
    assert!(matches!(
        fx.commands[..],
        [Command::ScrollLines { down: false, lines: 1, move_cursor: false }]
    ));
    assert_eq!(fx.mode, "visual");
}

#[test]
fn gd_asks_the_host_for_the_definition() {
    let mut s = at("let x = foo();", 0, 8);
    let fx = feed(&mut s, "gd");
    assert!(matches!(fx.commands[..], [Command::GotoDefinition]));
    assert_eq!((fx.mode, s.cursor(), fx.pending.as_str()), ("normal", Pos::new(0, 8), ""));
}

#[test]
fn visual_i_and_a_insert_once_at_the_selection_edges() {
    let text = "one two\n  three four\nfive six";
    // Made downward: `I` goes to the start of the first line.
    let mut s = at(text, 0, 2);
    let fx = feed(&mut s, "vjI");
    assert_eq!((fx.mode, s.cursor()), ("insert", Pos::new(0, 0)));
    // Made upward: `I` goes where the cursor is.
    let mut s = at(text, 1, 5);
    let fx = feed(&mut s, "vkI");
    assert_eq!((fx.mode, s.cursor()), ("insert", Pos::new(0, 5)));
    // `A` appends after the char the selection ends on, linewise or not.
    let mut s = at(text, 0, 2);
    let fx = feed(&mut s, "VjA");
    assert_eq!((fx.mode, s.cursor()), ("insert", Pos::new(1, 3)));
    let mut s = at(text, 0, 1);
    let fx = feed(&mut s, "vA");
    assert_eq!((fx.mode, s.cursor()), ("insert", Pos::new(0, 2)));
    // ...but before it when the selection ends in column 0.
    let mut s = at(text, 0, 0);
    let fx = feed(&mut s, "vjA");
    assert_eq!((fx.mode, s.cursor()), ("insert", Pos::new(1, 0)));
    let mut s = at(text, 0, 0);
    let fx = feed(&mut s, "VA");
    assert_eq!((fx.mode, s.cursor()), ("insert", Pos::new(0, 0)));
    // Ended with `$`: after the line's last char.
    let mut s = at(text, 0, 2);
    let fx = feed(&mut s, "vj$A");
    assert_eq!((fx.mode, s.cursor()), ("insert", Pos::new(1, 12)));
    // The selection is what `gv` brings back afterwards.
    feed(&mut s, "<esc>");
    let fx = feed(&mut s, "gv");
    assert_eq!((fx.mode, s.cursor()), ("visual", Pos::new(1, 11)));
}

#[test]
fn multi_cursor_number_and_case_keys_run_at_every_cursor() {
    let mut s = session("a 1\nb 1\nc 1");
    cursors(&mut s, &[(0, 0), (1, 0), (2, 0)]);
    let fx = feed(&mut s, "5<c-a>");
    assert_eq!(s.text(), "a 6\nb 6\nc 6");
    assert_eq!(actives(&fx), [(0, 2), (1, 2), (2, 2)]);
    let fx = feed(&mut s, "gUU");
    assert_eq!(s.text(), "A 6\nB 6\nC 6");
    assert_eq!(actives(&fx), [(0, 0), (1, 0), (2, 0)]);
    feed(&mut s, "g~iw");
    assert_eq!(s.text(), "a 6\nb 6\nc 6");
}

#[test]
fn multi_cursor_scroll_and_gd_run_once() {
    let mut s = session("a\nb\nc");
    cursors(&mut s, &[(0, 0), (1, 0), (2, 0)]);
    let fx = feed(&mut s, "<c-e>");
    assert!(matches!(
        fx.commands[..],
        [Command::ScrollLines { down: true, lines: 1, move_cursor: false }]
    ));
    let fx = feed(&mut s, "gd");
    assert!(matches!(fx.commands[..], [Command::GotoDefinition]));
    assert_eq!(actives(&fx), [(0, 0), (1, 0), (2, 0)]);
}

#[test]
fn multi_cursor_gv_reselects_at_every_cursor() {
    let mut s = session("one two\none two");
    s.set_cursors(
        &[(Pos::new(0, 4), Pos::new(0, 7)), (Pos::new(1, 4), Pos::new(1, 7))],
        true,
    );
    feed(&mut s, "<esc>0");
    let fx = feed(&mut s, "gv");
    assert_eq!(fx.mode, "visual");
    let sels: Vec<_> = fx
        .selections
        .iter()
        .map(|s| (s.anchor.line, s.anchor.col, s.active.line, s.active.col))
        .collect();
    assert_eq!(sels, [(0, 4, 0, 7), (1, 4, 1, 7)]);
}

// ---- easymotion ------------------------------------------------------------

/// The labels an effects painted: line, column and the keys left to press.
fn marks(fx: &Effects) -> Vec<(usize, usize, String)> {
    match fx.easy.as_ref() {
        Some(EasyUi::Labels { labels, .. }) => labels
            .iter()
            .map(|l| (l.line, l.col, l.text.clone()))
            .collect(),
        other => panic!("expected labels, got {other:?}"),
    }
}

#[test]
fn easymotion_labels_word_starts_and_jumps() {
    let mut s = session("foo bar baz\nqux quux");
    let fx = feed(&mut s, "<space><space>w");
    assert_eq!(
        marks(&fx),
        [
            (0, 4, "a".to_string()),
            (0, 8, "s".to_string()),
            (1, 0, "d".to_string()),
            (1, 4, "g".to_string()),
        ]
    );
    assert_eq!(s.cursor(), Pos::new(0, 0), "the cursor waits for the label");
    let fx = feed(&mut s, "d");
    assert_eq!(s.cursor(), Pos::new(1, 0));
    assert_eq!(fx.easy, Some(EasyUi::Done));
    assert_eq!(fx.pending, "");
}

#[test]
fn easymotion_backward_and_line_jumps_are_nearest_first() {
    let mut s = at("one two\nthree four\nfive six", 2, 0);
    // `b` labels the word starts behind the cursor, closest first.
    let fx = feed(&mut s, "<space><space>b");
    assert_eq!(
        marks(&fx),
        [
            (1, 6, "a".to_string()),
            (1, 0, "s".to_string()),
            (0, 4, "d".to_string()),
            (0, 0, "g".to_string()),
        ]
    );
    feed(&mut s, "d");
    assert_eq!(s.cursor(), Pos::new(0, 4));
    // `j` / `k` label whole lines, landing on the first non-blank.
    let mut s = at("  one\ntwo\n    three", 0, 3);
    let fx = feed(&mut s, "<space><space>j");
    assert_eq!(marks(&fx), [(1, 0, "a".to_string()), (2, 4, "s".to_string())]);
    feed(&mut s, "s");
    assert_eq!(s.cursor(), Pos::new(2, 4));
}

#[test]
fn easymotion_word_ends_forward_and_back() {
    let mut s = at("alpha beta gamma", 0, 7);
    let fx = feed(&mut s, "<space><space>e");
    assert_eq!(marks(&fx), [(0, 9, "a".to_string()), (0, 15, "s".to_string())]);
    feed(&mut s, "s");
    assert_eq!(s.cursor(), Pos::new(0, 15));
    // `ge` looks the other way.
    let mut s = at("alpha beta gamma", 0, 12);
    let fx = feed(&mut s, "<space><space>ge");
    assert_eq!(marks(&fx), [(0, 9, "a".to_string()), (0, 4, "s".to_string())]);
    feed(&mut s, "s");
    assert_eq!(s.cursor(), Pos::new(0, 4));
}

#[test]
fn easymotion_char_jumps_take_the_char_they_search_for() {
    let mut s = session("a.b.c.d");
    let fx = feed(&mut s, "<space><space>f.");
    assert_eq!(
        marks(&fx),
        [
            (0, 1, "a".to_string()),
            (0, 3, "s".to_string()),
            (0, 5, "d".to_string()),
        ]
    );
    feed(&mut s, "s");
    assert_eq!(s.cursor(), Pos::new(0, 3));
    // `s` looks both ways, nearest first, and `t` stops one char short.
    let mut s = at("a.b.c.d", 0, 3);
    let fx = feed(&mut s, "<space><space>s.");
    assert_eq!(marks(&fx), [(0, 1, "a".to_string()), (0, 5, "s".to_string())]);
    feed(&mut s, "a");
    assert_eq!(s.cursor(), Pos::new(0, 1));
    let mut s = session("a.b.c.d");
    let fx = feed(&mut s, "<space><space>t.");
    assert_eq!(s.cursor(), Pos::new(0, 0), "labels are up, nothing moved yet");
    assert_eq!(marks(&fx), [(0, 2, "a".to_string()), (0, 4, "s".to_string())]);
    feed(&mut s, "a");
    assert_eq!(s.cursor(), Pos::new(0, 2));
}

#[test]
fn easymotion_counted_char_jump_takes_that_many_chars() {
    let mut s = session("foo bar\nfoo baz\nfoz qux");
    // `2s` is easymotion's two-character search.
    let fx = feed(&mut s, "<space><space>2sfo");
    assert_eq!(marks(&fx), [(1, 0, "a".to_string()), (2, 0, "s".to_string())]);
    feed(&mut s, "s");
    assert_eq!(s.cursor(), Pos::new(2, 0));
}

#[test]
fn easymotion_slash_jump_takes_as_many_chars_as_you_type() {
    let mut s = session("foo\nfoo\nfoo");
    let fx = feed(&mut s, "<space><space>/foo");
    assert_eq!(fx.pending, "<space><space>/foo");
    assert!(fx.easy.is_none(), "nothing is labelled until <cr>");
    let fx = feed(&mut s, "<cr>");
    assert_eq!(marks(&fx), [(1, 0, "a".to_string()), (2, 0, "s".to_string())]);
    feed(&mut s, "s");
    assert_eq!(s.cursor(), Pos::new(2, 0));
}

#[test]
fn easymotion_with_one_target_jumps_without_asking() {
    let mut s = session("foo bar");
    let fx = feed(&mut s, "<space><space>w");
    assert_eq!(s.cursor(), Pos::new(0, 4));
    assert_eq!(fx.easy, Some(EasyUi::Done));
}

#[test]
fn easymotion_reports_when_there_is_nowhere_to_go() {
    let mut s = at("foo bar", 0, 4);
    let fx = feed(&mut s, "<space><space>w");
    assert_eq!(s.cursor(), Pos::new(0, 4));
    assert_eq!(fx.message.as_deref(), Some("no targets on screen"));
    assert_eq!(fx.easy, Some(EasyUi::Done));
}

#[test]
fn easymotion_labels_only_the_lines_on_screen() {
    let mut s = session("one two\nthree four\nfive six");
    s.set_view(0, 1);
    let fx = feed(&mut s, "<space><space>w");
    assert_eq!(
        marks(&fx),
        [
            (0, 4, "a".to_string()),
            (1, 0, "s".to_string()),
            (1, 6, "d".to_string()),
        ]
    );
    match fx.easy.as_ref() {
        Some(EasyUi::Labels { first_line, last_line, .. }) => {
            assert_eq!((*first_line, *last_line), (0, 1));
        }
        other => panic!("expected labels, got {other:?}"),
    }
}

#[test]
fn easymotion_two_key_labels_narrow_as_you_type() {
    let mut s = session("a b c d");
    // Two marker keys: the nearest target keeps a one-key label, the rest
    // share the other key as a prefix.
    s.set_easy_motion(true, "<space><space>", "ab");
    let fx = feed(&mut s, "<space><space>w");
    assert_eq!(
        marks(&fx),
        [
            (0, 2, "a".to_string()),
            (0, 4, "ba".to_string()),
            (0, 6, "bb".to_string()),
        ]
    );
    // The prefix leaves only its own group up, showing what is still to type.
    let fx = feed(&mut s, "b");
    assert_eq!(marks(&fx), [(0, 4, "a".to_string()), (0, 6, "b".to_string())]);
    assert_eq!(fx.pending, "<space><space>wb");
    feed(&mut s, "b");
    assert_eq!(s.cursor(), Pos::new(0, 6));
}

#[test]
fn easymotion_feeds_operators_and_visual_mode() {
    // `d` + a jump deletes up to the target, exclusive like `w` itself.
    let mut s = session("foo bar baz");
    feed(&mut s, "d<space><space>ws");
    assert_eq!(s.text(), "baz");
    assert_eq!(s.cursor(), Pos::new(0, 0));
    // A line jump is linewise wherever it lands.
    let mut s = session("one\ntwo\nthree\nfour");
    feed(&mut s, "d<space><space>js");
    assert_eq!(s.text(), "four");
    // A char jump follows `f`: inclusive going forward.
    let mut s = session("a.b.c.d");
    feed(&mut s, "d<space><space>f.s");
    assert_eq!(s.text(), "c.d");
    // In visual mode the jump extends the selection.
    let mut s = session("foo bar baz");
    let fx = feed(&mut s, "v<space><space>ws");
    assert_eq!(s.mode_label(), "visual");
    assert_eq!(actives(&fx), [(0, 9)]);
}

#[test]
fn easymotion_cancels_on_escape_and_on_a_stray_key() {
    let mut s = session("foo bar baz");
    let fx = feed(&mut s, "<space><space>w<esc>");
    assert_eq!(fx.easy, Some(EasyUi::Done));
    assert_eq!(fx.pending, "");
    assert_eq!(s.cursor(), Pos::new(0, 0));
    // A key that spells no label ends the jump instead of guessing.
    let fx = feed(&mut s, "<space><space>wz");
    assert_eq!(fx.easy, Some(EasyUi::Done));
    assert_eq!(s.cursor(), Pos::new(0, 0));
    // So does a motion key that names no jump.
    let fx = feed(&mut s, "<space><space>q");
    assert_eq!(fx.easy, Some(EasyUi::Done));
    assert_eq!(fx.pending, "");
}

#[test]
fn a_lone_leader_still_moves_right() {
    // Vim runs the keys it buffered when the next one completes no mapping;
    // `<space>l` is two characters right, not a jump.
    let mut s = session("abcdef");
    let fx = feed(&mut s, "<space>");
    assert_eq!(fx.pending, "<space>");
    assert_eq!(s.cursor(), Pos::new(0, 0));
    let fx = feed(&mut s, "l");
    assert_eq!(s.cursor(), Pos::new(0, 2));
    assert_eq!(fx.pending, "");
    // And it still feeds an operator: `d<space>` deletes the char the space
    // moves over, and the `l` that broke the match runs after it.
    let mut s = session("abcdef");
    feed(&mut s, "d<space>l");
    assert_eq!(s.text(), "bcdef");
    assert_eq!(s.cursor(), Pos::new(0, 1));
}

#[test]
fn easymotion_leaves_the_other_prompts_alone() {
    // A space inside a search or an ex command line is a space.
    let mut s = session("foo bar\nbaz foo bar");
    feed(&mut s, "/foo bar<cr>");
    assert_eq!(s.cursor(), Pos::new(1, 4));
    let mut s = session("a b\nc d");
    feed(&mut s, ":s/a b/x y/<cr>");
    assert_eq!(s.text(), "x y\nc d");
}

#[test]
fn easymotion_is_configurable_and_can_be_turned_off() {
    let mut s = session("foo bar baz");
    s.set_easy_motion(true, ",,", "jkl");
    // The old trigger is just a motion again.
    feed(&mut s, "<space>");
    assert_eq!(s.cursor(), Pos::new(0, 1));
    let fx = feed(&mut s, ",,w");
    assert_eq!(marks(&fx), [(0, 4, "j".to_string()), (0, 8, "k".to_string())]);
    feed(&mut s, "k");
    assert_eq!(s.cursor(), Pos::new(0, 8));

    let mut s = session("foo bar baz");
    s.set_easy_motion(false, "<space><space>", "asd");
    feed(&mut s, "<space><space>");
    assert_eq!(s.cursor(), Pos::new(0, 2), "space is a plain motion again");
}

#[test]
fn easymotion_stays_out_of_the_way_of_multiple_cursors() {
    // A jump is one cursor's choice of one landing place; with several of
    // them the trigger is the motion it otherwise is.
    let mut s = session("abc\nabc");
    cursors(&mut s, &[(0, 0), (1, 0)]);
    let fx = feed(&mut s, "<space><space>");
    assert_eq!(actives(&fx), [(0, 2), (1, 2)]);
}

// ---- registers ---------------------------------------------------------------

/// What register `name` holds, as `(text, linewise)`.
fn reg(s: &Session, name: char) -> Option<(String, bool)> {
    s.register(name).map(|r| (r.text, r.linewise))
}

/// The rows of the listing `fx` asks the host to show, by label.
fn list_labels(fx: &Effects, title: &str) -> Vec<String> {
    let rows = fx.commands.iter().find_map(|c| match c {
        Command::ShowList { title: t, rows } if t == title => Some(rows),
        _ => None,
    });
    rows.expect("a listing").iter().map(|r| r.label.clone()).collect()
}

#[test]
fn named_registers_take_yanks_and_feed_puts() {
    let mut s = at("one\ntwo\nthree", 0, 0);
    feed(&mut s, "\"ayyj\"byy");
    assert_eq!(reg(&s, 'a'), Some(("one".into(), true)));
    feed(&mut s, "\"ap");
    assert_eq!(s.text(), "one\ntwo\none\nthree");
    // `""` is whichever register was written last; putting from "a only
    // read it.
    feed(&mut s, "P");
    assert_eq!(s.text(), "one\ntwo\ntwo\none\nthree");
    // A yank into a named register leaves "0 alone; one without a name
    // fills it.
    assert_eq!(reg(&s, '0'), None);
    feed(&mut s, "yy");
    assert_eq!(reg(&s, '0'), Some(("two".into(), true)));
    // The registers belong to the editor, not the document.
    let mut other = at("x", 0, 0);
    feed(&mut other, "\"ap");
    assert_eq!(other.text(), "x\none");
}

#[test]
fn uppercase_registers_append_the_way_vim_mixes_them() {
    let mut s = at("aa\nbb\ncc", 0, 0);
    // Lines after characters: the register turns linewise.
    feed(&mut s, "\"ayiw\"Ayy");
    assert_eq!(reg(&s, 'a'), Some(("aa\naa".into(), true)));
    // Characters after lines start a line of their own.
    feed(&mut s, "\"byyj\"Byiw");
    assert_eq!(reg(&s, 'b'), Some(("aa\nbb".into(), true)));
    // Characters after characters run on.
    feed(&mut s, "\"cyiwj\"Cyiw");
    assert_eq!(reg(&s, 'c'), Some(("bbcc".into(), false)));
    // `""` reads the whole of the register appended to.
    feed(&mut s, "p");
    assert_eq!(s.text(), "aa\nbb\ncbbccc");
}

#[test]
fn counts_before_and_after_the_register_multiply() {
    let mut s = at("1\n2\n3\n4\n5\n6\n7\n8", 0, 0);
    let fx = feed(&mut s, "2\"a3");
    assert_eq!(fx.pending, "2\"a3");
    feed(&mut s, "yy");
    assert_eq!(reg(&s, 'a'), Some(("1\n2\n3\n4\n5\n6".into(), true)));
    feed(&mut s, "\"b2yy");
    assert_eq!(reg(&s, 'b'), Some(("1\n2".into(), true)));
    // An operator in front of `"` is dropped, as in Vim.
    let fx = feed(&mut s, "d\"a");
    assert_eq!(fx.pending, "");
}

#[test]
fn line_deletes_shift_the_numbered_registers() {
    let mut s = at("a\nb\nc\nd\ne", 0, 0);
    feed(&mut s, "dddddd");
    assert_eq!(reg(&s, '1'), Some(("c".into(), true)));
    assert_eq!(reg(&s, '2'), Some(("b".into(), true)));
    assert_eq!(reg(&s, '3'), Some(("a".into(), true)));
    feed(&mut s, "\"3p");
    assert_eq!(s.text(), "d\na\ne");
    // Yanks go to "0 and small deletes to "-; neither touches "1.
    feed(&mut s, "yyx");
    assert_eq!(reg(&s, '0'), Some(("a".into(), true)));
    assert_eq!(reg(&s, '-'), Some(("a".into(), false)));
    assert_eq!(reg(&s, '1'), Some(("c".into(), true)));
    // A named delete of lines fills "1 as well.
    feed(&mut s, "gg\"zdd");
    assert_eq!(reg(&s, 'z'), Some(("d".into(), true)));
    assert_eq!(reg(&s, '1'), Some(("d".into(), true)));
    assert_eq!(reg(&s, '2'), Some(("c".into(), true)));
}

#[test]
fn some_motions_always_delete_into_register_one() {
    // `:help quote_number`: `%`, `(`, `)`, `` ` ``, `/`, `?`, `n`, `N`, `{`
    // and `}` fill "1 even within a line — and "-, there being no name.
    let mut s = at("(a) b\nx", 0, 0);
    feed(&mut s, "d%");
    assert_eq!(s.text(), " b\nx");
    assert_eq!(reg(&s, '1'), Some(("(a)".into(), false)));
    assert_eq!(reg(&s, '-'), Some(("(a)".into(), false)));
    let mut s = at("foo bar baz", 0, 0);
    feed(&mut s, "d/baz<cr>");
    assert_eq!(reg(&s, '1'), Some(("foo bar ".into(), false)));
    feed(&mut s, "dw");
    assert_eq!(reg(&s, '-'), Some(("baz".into(), false)));
    assert_eq!(reg(&s, '1'), Some(("foo bar ".into(), false)));
}

#[test]
fn the_black_hole_keeps_every_register() {
    let mut s = at("one\ntwo", 0, 0);
    feed(&mut s, "yy\"_ddp");
    assert_eq!(s.text(), "two\none");
    assert_eq!(reg(&s, '1'), None);
    let fx = feed(&mut s, "\"_p");
    assert_eq!(fx.message.as_deref(), Some("E353: Nothing in register _"));
}

#[test]
fn x_s_d_c_x_and_y_take_a_register() {
    let mut s = at("abcdef", 0, 2);
    feed(&mut s, "\"aX\"bx");
    assert_eq!(s.text(), "adef");
    assert_eq!((reg(&s, 'a'), reg(&s, 'b')), (Some(("b".into(), false)), Some(("c".into(), false))));
    feed(&mut s, "\"cD");
    assert_eq!(reg(&s, 'c'), Some(("def".into(), false)));
    feed(&mut s, "\"dY");
    assert_eq!(reg(&s, 'd'), Some(("a".into(), true)));
    feed(&mut s, "\"es");
    assert_eq!((s.text().as_str(), reg(&s, 'e')), ("", Some(("a".into(), false))));
    let mut s = at("one\ntwo", 0, 1);
    feed(&mut s, "\"fC<esc>j\"gS<esc>");
    assert_eq!(reg(&s, 'f'), Some(("ne".into(), false)));
    assert_eq!(reg(&s, 'g'), Some(("two".into(), true)));
    // Visual mode too.
    let mut s = at("one two", 0, 0);
    feed(&mut s, "viw\"hy");
    assert_eq!(reg(&s, 'h'), Some(("one".into(), false)));
}

#[test]
fn read_only_registers_can_be_put_but_not_filled() {
    let mut s = at("abc", 0, 0);
    feed(&mut s, "i");
    type_ins(&mut s, "xy");
    feed(&mut s, "<esc>");
    assert_eq!(reg(&s, '.'), Some(("xy".into(), false)));
    ex(&mut s, "s/b/B/");
    assert_eq!(reg(&s, ':'), Some(("s/b/B/".into(), false)));
    feed(&mut s, "/c<cr>");
    assert_eq!(reg(&s, '/'), Some(("c".into(), false)));
    // A yank or delete into one does nothing at all.
    // (The second `y` of `".yy` is a `y` of its own, as in Vim.)
    feed(&mut s, "\".yy<esc>\":x");
    assert_eq!((s.text().as_str(), reg(&s, '0')), ("xyaBc", None));
    feed(&mut s, "\"/P");
    assert_eq!(s.text(), "xyaBcc");
    feed(&mut s, "\".p");
    assert_eq!(s.text(), "xyaBcxyc");
    // `":` is the line run before this one.
    let fx = ex(&mut s, "reg :");
    assert_eq!(list_labels(&fx, ":registers"), ["\":"]);
}

#[test]
fn dot_register_follows_the_insert_the_host_types() {
    let mut s = at("foo bar", 0, 0);
    feed(&mut s, "cw");
    type_ins(&mut s, "baz");
    feed(&mut s, "<esc>");
    assert_eq!(reg(&s, '.'), Some(("baz".into(), false)));
    // `o`: the host opens the line, and typing starts where it lands.
    let mut s = at("one", 0, 0);
    feed(&mut s, "o");
    host_open_line(&mut s, 0, false, "  ");
    type_ins(&mut s, "two");
    feed(&mut s, "<esc>");
    assert_eq!(reg(&s, '.'), Some(("two".into(), false)));
    // Typing then backing up over it.
    feed(&mut s, "A");
    type_ins(&mut s, "xy");
    s.apply_change(Pos::new(1, 6), Pos::new(1, 7), "");
    s.set_position(1, 6);
    feed(&mut s, "<esc>");
    assert_eq!(reg(&s, '.'), Some(("x".into(), false)));
    // Moving the cursor while inserting starts over from there, as in Vim.
    feed(&mut s, "A");
    type_ins(&mut s, "q");
    s.set_position(0, 0);
    type_ins(&mut s, "r");
    feed(&mut s, "<esc>");
    assert_eq!(reg(&s, '.'), Some(("r".into(), false)));
}

#[test]
fn visual_put_files_the_selection_and_visual_capital_p_keeps_it() {
    let mut s = at("foo bar", 0, 0);
    feed(&mut s, "yiwwviwp");
    assert_eq!(s.text(), "foo foo");
    assert_eq!(reg(&s, '"'), Some(("bar".into(), false)));
    assert_eq!(reg(&s, '-'), Some(("bar".into(), false)));
    let mut s = at("foo bar", 0, 0);
    feed(&mut s, "yiwwviwP");
    assert_eq!(s.text(), "foo foo");
    assert_eq!(reg(&s, '"'), Some(("foo".into(), false)));
    // A named register is put, and the selection still goes to `""`.
    let mut s = at("foo bar baz", 0, 0);
    feed(&mut s, "\"ayiwwyiwwviw\"ap");
    assert_eq!(s.text(), "foo bar foo");
    assert_eq!(reg(&s, '"'), Some(("baz".into(), false)));
    // Nothing to put: nothing is deleted either.
    let fx = feed(&mut s, "viw\"qp");
    assert_eq!((s.text().as_str(), fx.message.as_deref()), ("foo bar foo", Some("E353: Nothing in register q")));
}

#[test]
fn ex_registers_delete_yank_and_put() {
    let mut s = at("one\n  two\nthree", 0, 0);
    ex(&mut s, "y a");
    ex(&mut s, "2y A");
    assert_eq!(reg(&s, 'a'), Some(("one\n  two".into(), true)));
    // `:pu` below a line, `:pu!` above it, `:0pu` above the first line; the
    // cursor lands on the first non-blank of the last line put.
    let mut s = at("one\n  two\nthree", 1, 0);
    ex(&mut s, "y b");
    ex(&mut s, "$pu b");
    assert_eq!((s.text().as_str(), s.cursor()), ("one\n  two\nthree\n  two", Pos::new(3, 2)));
    ex(&mut s, "1pu! b");
    assert_eq!((s.text().as_str(), s.cursor()), ("  two\none\n  two\nthree\n  two", Pos::new(0, 2)));
    ex(&mut s, "0pu");
    assert_eq!(s.text(), "  two\n  two\none\n  two\nthree\n  two");
    // A charwise register goes in as lines all the same.
    let mut s = at("x", 0, 0);
    assert!(s.set_register('c', "p\nq", false));
    ex(&mut s, "pu c");
    assert_eq!((s.text().as_str(), s.cursor()), ("x\np\nq", Pos::new(2, 0)));
    let fx = ex(&mut s, "pu z");
    assert_eq!(fx.message.as_deref(), Some("E353: Nothing in register z"));
    // `:d` with a read-only register is a `:d` with trailing characters.
    let fx = ex(&mut s, "d .");
    assert_eq!(fx.message.as_deref(), Some("E488: Trailing characters: ."));
}

#[test]
fn registers_command_lists_what_is_set() {
    let mut s = at("one\ntwo", 0, 0);
    feed(&mut s, "\"ayyjyy");
    let fx = ex(&mut s, "reg");
    assert_eq!(list_labels(&fx, ":registers"), ["\"\"", "\"0", "\"a"]);
    let Some(Command::ShowList { rows, .. }) = fx.commands.first() else { panic!("a listing") };
    assert_eq!((rows[2].description.as_str(), rows[2].detail.as_str()), ("line", "one^J"));
    let fx = ex(&mut s, "di a0");
    assert_eq!(list_labels(&fx, ":registers"), ["\"0", "\"a"]);
    let fx = ex(&mut s, "registers z");
    assert_eq!((fx.commands.len(), fx.message.as_deref()), (0, Some("the registers are empty")));
}

#[test]
fn registers_can_be_read_and_written_by_name() {
    // The API recordings (`q`) and `@` will use: a write leaves `""` and the
    // numbered registers alone; an uppercase name appends.
    let mut s = at("x", 0, 0);
    assert!(s.set_register('a', "hello", false));
    assert!(s.set_register('A', " world", false));
    assert_eq!(reg(&s, 'a'), Some(("hello world".into(), false)));
    assert_eq!(reg(&s, '"'), None);
    assert!(!s.set_register('.', "x", false));
    assert!(!s.set_register('_', "x", false));
    assert!(!s.set_register('!', "x", false));
    // `"/` is the pattern `n` repeats.
    let mut s = at("ab ab ab", 0, 0);
    assert!(s.set_register('/', "ab", false));
    feed(&mut s, "n");
    assert_eq!(s.cursor(), Pos::new(0, 3));
}

// ---- system clipboard ----------------------------------------------------------

fn clipboard_writes(fx: &Effects) -> Vec<String> {
    fx.commands
        .iter()
        .filter_map(|c| match c {
            Command::SetClipboard { text } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn clipboard_registers_write_through_the_host_and_read_what_it_supplies() {
    let mut s = at("one\ntwo", 0, 0);
    // Lines go on the clipboard with the line break that says so.
    let fx = feed(&mut s, "\"+yy");
    assert_eq!(clipboard_writes(&fx), ["one\n"]);
    let fx = feed(&mut s, "\"*yiw");
    assert_eq!(clipboard_writes(&fx), ["one"]);
    let fx = feed(&mut s, "yy");
    assert!(clipboard_writes(&fx).is_empty());
    // Only a key after `"+` / `"*` needs the clipboard read first.
    assert!(!s.wants_clipboard(Key::Char('p')));
    feed(&mut s, "\"+");
    assert!(s.wants_clipboard(Key::Char('p')));
    s.set_clipboard("from elsewhere\r\n");
    feed(&mut s, "p");
    assert_eq!(s.text(), "one\nfrom elsewhere\ntwo");
    assert!(!s.wants_clipboard(Key::Char('p')));
    // So does a `:pu +` or a `:reg` that shows it.
    s.key(Key::Char(':'));
    feed_literal(&mut s, "pu +");
    assert!(s.wants_clipboard(Key::Enter));
    s.key(Key::Esc);
    s.key(Key::Char(':'));
    feed_literal(&mut s, "reg a");
    assert!(!s.wants_clipboard(Key::Enter));
    s.key(Key::Esc);
    s.set_clipboard("word");
    ex(&mut s, "pu *");
    assert_eq!(s.text(), "one\nfrom elsewhere\nword\ntwo");
}

#[test]
fn clipboard_is_written_once_for_every_cursor() {
    let mut s = session("one\ntwo");
    cursors(&mut s, &[(0, 0), (1, 0)]);
    let fx = feed(&mut s, "\"+yy");
    assert_eq!(clipboard_writes(&fx), ["one\ntwo\n"]);
}

// ---- marks -------------------------------------------------------------------

#[test]
fn marks_are_set_with_m_and_reached_with_quote_and_backtick() {
    let mut s = at("  alpha\n  beta\ngamma", 0, 4);
    feed(&mut s, "majj");
    feed(&mut s, "'a");
    assert_eq!(s.cursor(), Pos::new(0, 2));
    feed(&mut s, "G`a");
    assert_eq!(s.cursor(), Pos::new(0, 4));
    let fx = feed(&mut s, "j'b");
    assert_eq!((fx.message.as_deref(), s.cursor()), (Some("E20: Mark not set"), Pos::new(1, 4)));
    let fx = feed(&mut s, "'!");
    assert_eq!(fx.message.as_deref(), Some("E78: Unknown mark"));
    assert_eq!(s.mark('a'), Some(Pos::new(0, 4)));
}

#[test]
fn mark_motions_feed_operators_and_selections() {
    // `'` is linewise.
    let mut s = at("one\ntwo\nthree\nfour", 1, 1);
    feed(&mut s, "majjd'a");
    assert_eq!(s.text(), "one");
    assert_eq!(reg(&s, '1'), Some(("two\nthree\nfour".into(), true)));
    // `` ` `` is exclusive, and one of the motions that fill "1.
    let mut s = at("hello world", 0, 6);
    feed(&mut s, "ma0d`a");
    assert_eq!(s.text(), "world");
    assert_eq!(reg(&s, '1'), Some(("hello ".into(), false)));
    let mut s = at("hello world", 0, 6);
    feed(&mut s, "ma0y`a");
    assert_eq!(reg(&s, '0'), Some(("hello ".into(), false)));
    // In visual mode they move the cursor end of the selection.
    let mut s = at("hello world", 0, 2);
    feed(&mut s, "ma$v`ad");
    assert_eq!(s.text(), "he");
}

#[test]
fn marks_follow_the_text_they_were_set_on() {
    let mut s = at("a\nb\nc\nd", 2, 0);
    feed(&mut s, "maggdd");
    assert_eq!(s.mark('a'), Some(Pos::new(1, 0)));
    feed(&mut s, "'a");
    assert_eq!(s.cursor(), Pos::new(1, 0));
    // Its own line deleted, the mark goes with it.
    let fx = feed(&mut s, "dd'a");
    assert_eq!((s.mark('a'), fx.message.as_deref()), (None, Some("E20: Mark not set")));
    // Lines put in above carry it down; typing in front of it on its own
    // line leaves its column, as Vim does — and the host's edits count too.
    let mut s = at("xyz\nabc", 1, 2);
    feed(&mut s, "mb");
    s.apply_change(Pos::new(0, 0), Pos::new(0, 0), "new\n");
    s.apply_change(Pos::new(2, 0), Pos::new(2, 0), "__");
    assert_eq!(s.mark('b'), Some(Pos::new(2, 2)));
    // A join takes it along to its place in the joined line.
    let mut s = at("aaa\nbbb\nccc", 1, 1);
    feed(&mut s, "mdkJ");
    assert_eq!(s.mark('d'), Some(Pos::new(0, 5)));
    let mut s = at("aaa\n  bbb", 1, 3);
    // (`:ke` would be `:keepmarks`, so mark e is set with a space.)
    ex(&mut s, "k e");
    ex(&mut s, "1,2j!");
    assert_eq!(s.mark('e'), Some(Pos::new(0, 5)));
    // Lines moved with `:m` take their marks with them.
    let mut s = at("1\n2\n3\n4", 0, 0);
    feed(&mut s, "majmb");
    ex(&mut s, "m$");
    assert_eq!(s.text(), "1\n3\n4\n2");
    assert_eq!((s.mark('a'), s.mark('b')), (Some(Pos::new(0, 0)), Some(Pos::new(3, 0))));
    ex(&mut s, "1m2");
    assert_eq!((s.mark('a'), s.mark('b')), (Some(Pos::new(1, 0)), Some(Pos::new(3, 0))));
    // `cc` keeps the first line's marks and takes the rest's.
    let mut s = at("1\n2\n3", 0, 0);
    feed(&mut s, "majmbk2cc<esc>");
    assert_eq!((s.mark('a'), s.mark('b')), (Some(Pos::new(0, 0)), None));
}

#[test]
fn jumps_set_the_previous_context_mark() {
    let mut s = at("1\n2\n3\n4\n5\n6\n7\n8\n9\n10", 7, 0);
    feed(&mut s, "2G''");
    assert_eq!(s.cursor().line, 7);
    feed(&mut s, "''");
    assert_eq!(s.cursor().line, 1);
    // A jump that went nowhere puts the old `''` back (mark.c checkpcmark).
    feed(&mut s, "10GG''");
    assert_eq!(s.cursor().line, 1);
    // `` ` `` `` ` `` goes back to the very column.
    feed(&mut s, "$G``");
    assert_eq!(s.cursor(), Pos::new(1, 0));
    // Searches, `%`, `{ }`, `( )`, `H M L` and `:N` are jumps; `j` and `w`
    // are not. Each step moves, and leaves `''` where it came from if it is
    // a jump.
    let mut s = at("(1\n2\n3)\n\n2\n6. Seven.", 0, 0);
    let steps = [
        ("/6<cr>", true),
        ("?2<cr>", true),
        ("n", true),
        ("k", false),
        ("%", true),
        ("}", true),
        ("{", true),
        ("L", true),
        ("H", true),
        (")", true),
        ("j", false),
        ("w", false),
        ("M", true),
    ];
    for (keys, jump) in steps {
        let (before, context) = (s.cursor(), s.mark('\''));
        feed(&mut s, keys);
        assert_ne!(s.cursor(), before, "{keys} moves");
        assert_eq!(s.mark('\''), if jump { Some(before) } else { context }, "{keys}");
    }
    let before = s.cursor();
    ex(&mut s, "5");
    assert_eq!(s.mark('\''), Some(before));
    // With an operator too, and `d''` is linewise.
    let mut s = at("a\nb\nc\nd", 0, 0);
    feed(&mut s, "3Gd''");
    assert_eq!(s.text(), "d");
}

#[test]
fn ctrl_o_and_ctrl_i_walk_the_jumplist() {
    // Vim's own answers for this sequence (gg 3G 5G 8G, then ^O ^O ^I ^I ^I).
    let mut s = at("1\n2\n3\n4\n5\n6\n7\n8\n9\n10", 0, 0);
    feed(&mut s, "gg3G5G8G");
    let mut lines = Vec::new();
    for key in ["<c-o>", "<c-o>", "<c-i>", "<c-i>", "<c-i>"] {
        feed(&mut s, key);
        lines.push(s.cursor().line + 1);
    }
    assert_eq!(lines, [5, 3, 5, 8, 8]);
    feed(&mut s, "2<c-o>");
    assert_eq!(s.cursor().line + 1, 3);
    // An operator does not take them.
    let fx = feed(&mut s, "d<c-o>");
    assert_eq!((s.text().lines().count(), fx.pending.as_str()), (10, ""));
    // Walking the list leaves `''` alone.
    let mut s = at("1\n2\n3", 0, 0);
    feed(&mut s, "3G<c-o>");
    assert_eq!(s.mark('\''), Some(Pos::new(2, 0)));
}

#[test]
fn the_jumplist_keeps_one_entry_per_line_and_a_hundred_at_most() {
    let text: Vec<String> = (1..=200).map(|n| n.to_string()).collect();
    let mut s = at(&text.join("\n"), 0, 0);
    for line in 1..=150 {
        feed(&mut s, &format!("{line}G"));
    }
    let fx = ex(&mut s, "jumps");
    let labels = list_labels(&fx, ":jumps");
    assert_eq!(labels.len(), 101, "a hundred entries and the place after them");
    assert_eq!((labels[0].as_str(), labels[100].as_str()), ("100", ">"));
    // Jumping back and forth between two lines leaves one entry for each.
    let mut s = at("1\n2\n3", 0, 0);
    feed(&mut s, "3Ggg3Ggg3G");
    let fx = ex(&mut s, "ju");
    assert_eq!(list_labels(&fx, ":jumps"), ["2", "1", ">"]);
}

#[test]
fn special_marks_track_changes_inserts_and_selections() {
    let mut s = at("one two three", 0, 4);
    feed(&mut s, "yiw");
    assert_eq!((s.mark('['), s.mark(']')), (Some(Pos::new(0, 4)), Some(Pos::new(0, 6))));
    feed(&mut s, "$p");
    assert_eq!((s.mark('['), s.mark(']')), (Some(Pos::new(0, 13)), Some(Pos::new(0, 15))));
    feed(&mut s, "0x");
    assert_eq!((s.mark('.'), s.mark('['), s.mark(']')), (Some(Pos::new(0, 0)), Some(Pos::new(0, 0)), Some(Pos::new(0, 0))));
    feed(&mut s, "A");
    type_ins(&mut s, "!!");
    feed(&mut s, "<esc>");
    assert_eq!(s.mark('^'), Some(Pos::new(0, 17)));
    assert_eq!((s.mark('['), s.mark(']')), (Some(Pos::new(0, 15)), Some(Pos::new(0, 16))));
    assert_eq!(s.mark('.'), Some(Pos::new(0, 15)));
    feed(&mut s, "0lvll<esc>");
    assert_eq!((s.mark('<'), s.mark('>')), (Some(Pos::new(0, 1)), Some(Pos::new(0, 3))));
    // `:s` leaves `'.` on the last line it changed, and `'[ ']` around them.
    let mut t = at("a1\nb\na2\na3", 0, 0);
    ex(&mut t, "%s/a/x\\ry/");
    assert_eq!(t.text(), "x\ny1\nb\nx\ny2\nx\ny3");
    assert_eq!((t.mark('.'), t.mark('['), t.mark(']')), (Some(Pos::new(5, 0)), Some(Pos::new(0, 0)), Some(Pos::new(6, 0))));
    // While a selection is up, `'<` is still the last one's.
    feed(&mut s, "$v`<");
    assert_eq!(s.cursor(), Pos::new(0, 1));
    feed(&mut s, "<esc>");
    assert_eq!((s.mark('<'), s.mark('>')), (Some(Pos::new(0, 1)), Some(Pos::new(0, 16))));
    // `m[` and `m<` move them by hand.
    feed(&mut s, "0m[");
    assert_eq!(s.mark('['), Some(Pos::new(0, 0)));
    feed(&mut s, "m<");
    assert_eq!(s.mark('<'), Some(Pos::new(0, 0)));
    // Lines yanked: `']` is the last line's last character.
    let mut s = at("ab\ncde", 0, 0);
    feed(&mut s, "yj");
    assert_eq!((s.mark('['), s.mark(']')), (Some(Pos::new(0, 0)), Some(Pos::new(1, 2))));
}

#[test]
fn gi_and_gv_follow_the_edits_made_since() {
    let mut s = at("a\nb", 1, 0);
    feed(&mut s, "A");
    type_ins(&mut s, "x");
    feed(&mut s, "<esc>vh<esc>ggdd");
    let fx = feed(&mut s, "gv");
    assert_eq!((fx.mode, s.cursor(), s.mark('<')), ("visual", Pos::new(0, 0), Some(Pos::new(0, 0))));
    feed(&mut s, "<esc>gi");
    assert_eq!((s.mode_label(), s.cursor()), ("insert", Pos::new(0, 2)));
}

#[test]
fn ex_addresses_and_commands_for_marks() {
    let mut s = at("a\nb\nc\nd\ne", 1, 0);
    feed(&mut s, "majjmb");
    ex(&mut s, "'a,'bd");
    assert_eq!(s.text(), "a\ne");
    let fx = ex(&mut s, "'b");
    assert_eq!(fx.message.as_deref(), Some("E20: Mark not set"));
    // `:k` and `:mark` set one on a line's first non-blank.
    let mut s = at("x\n  y\nz", 0, 0);
    ex(&mut s, "2ka");
    ex(&mut s, "3mark b");
    assert_eq!((s.mark('a'), s.mark('b')), (Some(Pos::new(1, 2)), Some(Pos::new(2, 0))));
    ex(&mut s, "'a");
    assert_eq!(s.cursor(), Pos::new(1, 2));
    let fx = ex(&mut s, "mark 1");
    assert_eq!(fx.message.as_deref(), Some("E191: Argument must be a letter or forward/backward quote"));
    // `:marks` lists them; `:delm` deletes some, `:delm!` all of them.
    let fx = ex(&mut s, "marks");
    assert_eq!(list_labels(&fx, ":marks"), ["'", "a", "b"]);
    ex(&mut s, "delm a");
    assert_eq!(s.mark('a'), None);
    let fx = ex(&mut s, "marks ab");
    assert_eq!(list_labels(&fx, ":marks"), ["b"]);
    ex(&mut s, "delm!");
    let fx = ex(&mut s, "marks b");
    assert_eq!(fx.message.as_deref(), Some("E283: No marks matching \"b\""));
    feed(&mut s, "mamcme");
    ex(&mut s, "delm a-c");
    assert_eq!((s.mark('a'), s.mark('c'), s.mark('e').is_some()), (None, None, true));
    let fx = ex(&mut s, "delm");
    assert_eq!(fx.message.as_deref(), Some("E471: Argument required"));
}

#[test]
fn file_marks_reach_across_documents_through_the_host() {
    let mut a = at("one\ntwo", 1, 1);
    a.set_document("file:///a.txt");
    let mut b = at("x", 0, 0);
    b.set_document("file:///b.txt");
    feed(&mut a, "mA");
    let fx = feed(&mut b, "'A");
    let goto = |fx: &Effects| {
        fx.commands.iter().find_map(|c| match c {
            Command::GotoMark { uri, line, col, first_non_blank } => Some((uri.clone(), *line, *col, *first_non_blank)),
            _ => None,
        })
    };
    assert_eq!(goto(&fx), Some(("file:///a.txt".into(), 1, 1, true)));
    let fx = feed(&mut b, "`A");
    assert_eq!(goto(&fx), Some(("file:///a.txt".into(), 1, 1, false)));
    // An operator cannot reach into another document.
    let fx = feed(&mut b, "d'A");
    assert_eq!((goto(&fx), fx.message.as_deref()), (None, Some("E20: Mark not set")));
    // In its own document it is a mark like the others, following the edits.
    a.apply_change(Pos::new(0, 0), Pos::new(0, 0), "zero\n");
    feed(&mut a, "gg`A");
    assert_eq!(a.cursor(), Pos::new(2, 1));
    let fx = ex(&mut b, "marks A");
    let Some(Command::ShowList { rows, .. }) = fx.commands.first() else { panic!("a listing") };
    assert_eq!((rows[0].uri.as_deref(), rows[0].description.as_str()), (Some("file:///a.txt"), "line 3, col 1"));
    ex(&mut b, "delm A");
    let fx = feed(&mut b, "'A");
    assert_eq!(fx.message.as_deref(), Some("E20: Mark not set"));
}

// ---- registers and marks with several cursors -------------------------------

#[test]
fn multi_cursor_registers_hold_a_piece_per_cursor() {
    let mut s = session("one\ntwo\nthree");
    cursors(&mut s, &[(0, 0), (1, 0), (2, 0)]);
    feed(&mut s, "\"ayy");
    let a = s.register('a').expect("filled");
    assert_eq!((a.text.as_str(), a.linewise, a.parts.len()), ("one\ntwo\nthree", true, 3));
    // At as many cursors, each puts its own piece.
    feed(&mut s, "\"ap");
    assert_eq!(s.text(), "one\none\ntwo\ntwo\nthree\nthree");
    // At one cursor, every piece.
    feed(&mut s, "<esc>G\"ap");
    assert_eq!(s.text(), "one\none\ntwo\ntwo\nthree\nthree\none\ntwo\nthree");
}

#[test]
fn multi_cursor_deletes_shift_the_numbered_registers_once() {
    let mut s = at("x\na\nb\nc", 0, 0);
    feed(&mut s, "dd");
    cursors(&mut s, &[(0, 0), (2, 0)]);
    feed(&mut s, "dd");
    assert_eq!(s.text(), "b");
    assert_eq!(reg(&s, '1'), Some(("a\nc".into(), true)));
    assert_eq!(reg(&s, '2'), Some(("x".into(), true)));
}

#[test]
fn multi_cursor_visual_put_puts_what_was_yanked_everywhere() {
    let mut s = at("foo\nbar\nbaz", 0, 0);
    feed(&mut s, "yiw");
    s.set_cursors(&[(Pos::new(1, 0), Pos::new(1, 3)), (Pos::new(2, 0), Pos::new(2, 3))], true);
    feed(&mut s, "p");
    assert_eq!(s.text(), "foo\nfoo\nfoo");
}

#[test]
fn multi_cursor_marks_come_from_the_primary() {
    let mut s = session("a\nb\nc");
    cursors(&mut s, &[(1, 0), (2, 0)]);
    feed(&mut s, "mz");
    assert_eq!(s.mark('z'), Some(Pos::new(1, 0)));
    // Every cursor's edits carry the marks along.
    feed(&mut s, "O");
    let mut s = session("a\nb\nc\nd");
    feed(&mut s, "Gmz");
    cursors(&mut s, &[(0, 0), (1, 0)]);
    feed(&mut s, "dd");
    assert_eq!(s.mark('z'), Some(Pos::new(1, 0)));
}

#[test]
fn ctrl_o_with_several_cursors_drops_back_to_one() {
    let mut s = at("1\n2\n3\n4", 0, 0);
    feed(&mut s, "3G");
    cursors(&mut s, &[(2, 0), (3, 0)]);
    let fx = feed(&mut s, "<c-o>");
    assert_eq!(actives(&fx), [(0, 0)]);
}

// ---- dot-repeat and macros ---------------------------------------------------

/// Run what `fx` asks the host to run (`Command::Execute`) the way the host
/// does: a key at a time through `feed`, the plain characters of insert mode
/// through `insert_text`, a register run from inside one in front of what is
/// left of it, the line-insert of `o` / `O` done as `host_open_line` does it
/// (keeping the indent of the line it opens from), and the whole run stopped
/// by a key that fails. Returns how many keys ran.
fn execute(s: &mut Session, fx: &Effects) -> usize {
    let mut frames: Vec<(Vec<String>, usize, usize)> = Vec::new();
    let mut ran = 0;
    let follow = |s: &mut Session, fx: &Effects, frames: &mut Vec<(Vec<String>, usize, usize)>| {
        for c in &fx.commands {
            match c {
                Command::Execute { keys, count } => frames.push((keys.clone(), 0, *count)),
                Command::OpenLine { above } => {
                    let line = s.cursor().line;
                    let text = s.text();
                    let current = text.split('\n').nth(line).unwrap_or("");
                    let indent = &current[..current.len() - current.trim_start().len()];
                    host_open_line(s, line, *above, indent);
                }
                _ => {}
            }
        }
    };
    follow(s, fx, &mut frames);
    while let Some(top) = frames.last_mut() {
        if top.1 == top.0.len() {
            top.2 -= 1;
            top.1 = 0;
            if top.2 == 0 || top.0.is_empty() {
                frames.pop();
            }
            continue;
        }
        let key = top.0[top.1].clone();
        top.1 += 1;
        ran += 1;
        assert!(ran < 10_000, "runaway macro");
        let fx = if s.mode_label() == "insert" && key != "<esc>" {
            let text = match key.as_str() {
                "<cr>" => "\n".to_string(),
                "<space>" => " ".to_string(),
                k => k.to_string(),
            };
            s.insert_text(&text)
        } else {
            s.feed(Key::parse(&key).expect("a key"))
        };
        if fx.failed {
            break;
        }
        follow(s, &fx, &mut frames);
    }
    ran
}

/// `feed`, running whatever a key asks the host to run as it goes.
fn play(s: &mut Session, keys: &str) -> Effects {
    let mut last = None;
    for key in vim_engine::keys::parse_sequence(keys) {
        let fx = s.key(key);
        execute(s, &fx);
        last = Some(fx);
    }
    last.expect("at least one key")
}

const LINES: &str = "one two three four 7\ntwo three four five 8\nsix seven eight nine 9";

#[test]
fn dot_repeats_standalone_changes() {
    // Every expectation is Vim's own, from `vim -u NONE` on the same text.
    let cases = [
        ("2xw.", "e o three four 7", (0, 2)),
        ("w2X.", "two three four 7", (0, 0)),
        ("r-w.", "--e two three four 7", (0, 1)),
        ("3rxw.", "xxx xxx three four 7", (0, 6)),
        ("yiwwPw.", "one onetwo onethree four 7", (0, 13)),
        ("yiwp.", "ooneonene two three four 7", (0, 6)),
        ("~~.", "ONE two three four 7", (0, 3)),
        ("3~.", "ONE TWo three four 7", (0, 6)),
        ("<c-a>.", "one two three four 9", (0, 19)),
        ("3<c-a>2.", "one two three four 12", (0, 20)),
        ("wgUiww.", "one TWO THREE four 7", (0, 8)),
    ];
    for (keys, first, cursor) in cases {
        let mut s = session(LINES);
        feed(&mut s, keys);
        let text = s.text();
        assert_eq!(
            (text.lines().next().unwrap(), s.cursor()),
            (first, Pos::new(cursor.0, cursor.1)),
            "{keys}"
        );
    }
    // An undo is no change: the `x` before it is what `.` repeats. The
    // undo itself is the host's, whose change mirrors back.
    let mut s = session(LINES);
    feed(&mut s, "x");
    assert!(matches!(feed(&mut s, "u").commands[..], [Command::Undo]));
    s.apply_change(Pos::new(0, 0), Pos::new(0, 0), "o");
    s.set_position(0, 0);
    feed(&mut s, ".");
    assert_eq!(s.text().lines().next(), Some("ne two three four 7"));
    let mut s = session(LINES);
    feed(&mut s, "wDj.");
    assert_eq!((s.text().as_str(), s.cursor()), ("one \ntwo\nsix seven eight nine 9", Pos::new(1, 2)));
    let mut s = session(LINES);
    feed(&mut s, "J.");
    assert_eq!(s.text(), "one two three four 7 two three four five 8 six seven eight nine 9");
    assert_eq!(s.cursor(), Pos::new(0, 42));
    let mut s = session(LINES);
    feed(&mut s, "5<c-x>j.");
    assert_eq!(s.text(), "one two three four 2\ntwo three four five 3\nsix seven eight nine 9");
    let mut s = session(LINES);
    feed(&mut s, "w~j0.");
    assert_eq!(s.text(), "one Two three four 7\nTwo three four five 8\nsix seven eight nine 9");
    // `gJ` on the last line fails, and the text stays as it was.
    let mut s = session(LINES);
    let fx = feed(&mut s, "gJj.");
    assert!(fx.failed);
    assert_eq!(s.text(), "one two three four 7two three four five 8\nsix seven eight nine 9");
    // `>j` asks the host to indent again.
    let mut s = session(LINES);
    feed(&mut s, ">j");
    let fx = feed(&mut s, ".");
    assert!(matches!(
        fx.commands[..],
        [Command::IndentLines { start_line: 0, end_line: 1, dedent: false, times: 1 }]
    ));
}

#[test]
fn dot_repeats_an_operator_with_its_count_and_register() {
    let mut s = session(LINES);
    feed(&mut s, "dwj.");
    assert_eq!(s.text(), "two three four 7\nthree four five 8\nsix seven eight nine 9");
    // A count after the operator repeats as one before it.
    let mut s = session(LINES);
    feed(&mut s, "d3wj.");
    assert_eq!(s.text(), "four 7\nfive 8\nsix seven eight nine 9");
    // A count given to `.` replaces it, for the repeats after it too.
    let mut s = session("a b c d e f g h i j k l");
    feed(&mut s, "d2w3..");
    assert_eq!(s.text(), "i j k l");
    // A named register stays the one named.
    let mut s = session(LINES);
    feed(&mut s, "\"ayiwj\"ap.");
    assert_eq!(s.text().lines().nth(1), Some("toneonewo three four five 8"));
    assert_eq!(s.cursor(), Pos::new(1, 6));
    // A numbered one steps on: `"1p..` puts `"1`, `"2`, `"3`.
    let mut s = session("a\nb\nc\nd\ne");
    feed(&mut s, "dddddddddd\"1p..");
    assert_eq!(s.text(), "\ne\nd\nc");
    assert_eq!(s.cursor(), Pos::new(3, 0));
}

#[test]
fn dot_ignores_what_changes_nothing() {
    // Motions, yanks and searches leave the change to repeat as it was.
    let mut s = session(LINES);
    feed(&mut s, "dwyyj");
    feed(&mut s, "/two<cr>");
    feed(&mut s, "gg0");
    feed(&mut s, ".");
    assert_eq!(s.text().lines().next(), Some("three four 7"));
    // So does a command line, which `.` does not repeat (`:help .`).
    let mut s = session(LINES);
    feed(&mut s, "x:s/t/T/<cr>.");
    assert_eq!(s.text().lines().next(), Some("e Two three four 7"));
    // With no change yet, `.` fails.
    let mut s = session(LINES);
    assert!(feed(&mut s, ".").failed);
}

#[test]
fn dot_repeats_a_change_with_what_its_insert_typed() {
    let mut s = session("foo bar foo baz");
    feed(&mut s, "cw");
    type_ins(&mut s, "xy");
    feed(&mut s, "<esc>w");
    let fx = feed(&mut s, ".");
    assert_eq!((s.text().as_str(), s.cursor()), ("xy xy foo baz", Pos::new(0, 4)));
    host_agrees("xy bar foo baz", &fx, &s);
    // The delete and the text typed in its place go to the host as one edit.
    let edits: Vec<_> = fx.edits.iter().map(|e| (e.start, e.end, e.text.as_str())).collect();
    assert_eq!(edits, [(Pos::new(0, 3), Pos::new(0, 6), "xy")]);
    assert_eq!(fx.mode, "normal");

    let mut s = session("foo bar foo baz");
    feed(&mut s, "ciw");
    type_ins(&mut s, "xy");
    feed(&mut s, "<esc>ww.");
    assert_eq!((s.text().as_str(), s.cursor()), ("xy bar xy baz", Pos::new(0, 8)));

    let mut s = session("a b c\nd e f");
    feed(&mut s, "A");
    type_ins(&mut s, ";");
    feed(&mut s, "<esc>j.");
    assert_eq!((s.text().as_str(), s.cursor()), ("a b c;\nd e f;", Pos::new(1, 5)));

    for keys in ["I", "gI"] {
        let mut s = session(LINES);
        feed(&mut s, keys);
        type_ins(&mut s, "foo ");
        feed(&mut s, "<esc>j.");
        assert_eq!(s.text().lines().nth(1), Some("foo two three four five 8"), "{keys}");
        assert_eq!(s.cursor(), Pos::new(1, 3), "{keys}");
    }

    let mut s = session(LINES);
    feed(&mut s, "S");
    type_ins(&mut s, "xy");
    feed(&mut s, "<esc>j.");
    assert_eq!((s.text().as_str(), s.cursor()), ("xy\nxy\nsix seven eight nine 9", Pos::new(1, 1)));

    let mut s = session(LINES);
    feed(&mut s, "s");
    type_ins(&mut s, "Q");
    feed(&mut s, "<esc>w.");
    assert_eq!((s.text().lines().next(), s.cursor()), (Some("Qne Qwo three four 7"), Pos::new(0, 4)));
    // `".` and `.` agree.
    assert_eq!(reg(&s, '.'), Some(("Q".into(), false)));
}

/// Apply `fx`'s edits to `before` the way the host does — all of them
/// against the document as it was before the key — and check that it comes
/// out as the engine's mirror did.
fn host_agrees(before: &str, fx: &Effects, s: &Session) {
    let mut edits: Vec<&Edit> = fx.edits.iter().collect();
    edits.sort_by_key(|e| std::cmp::Reverse((e.start, e.end)));
    let mut host = Buffer::from_text(before);
    for e in edits {
        host.apply_change(e.start, e.end, &e.text);
    }
    assert_eq!(host.text(), s.text(), "the host's copy");
}

#[test]
fn counted_inserts_repeat_their_text() {
    let mut s = session("abc");
    feed(&mut s, "3i");
    type_ins(&mut s, "x");
    let fx = feed(&mut s, "<esc>");
    assert_eq!((s.text().as_str(), s.cursor()), ("xxxabc", Pos::new(0, 2)));
    host_agrees("xabc", &fx, &s);
    let fx = feed(&mut s, ".");
    assert_eq!((s.text().as_str(), s.cursor()), ("xxxxxxabc", Pos::new(0, 4)));
    host_agrees("xxxabc", &fx, &s);
    assert_eq!(reg(&s, '.'), Some(("x".into(), false)));

    // The repeats go in where the cursor is, in the host's coordinates too.
    let mut s = session("ab cd");
    feed(&mut s, "2i");
    type_ins(&mut s, "xy");
    feed(&mut s, "<esc>w");
    let fx = feed(&mut s, ".");
    assert_eq!((s.text().as_str(), s.cursor()), ("xyxyab xyxycd", Pos::new(0, 10)));
    host_agrees("xyxyab cd", &fx, &s);

    let mut s = session("abc");
    feed(&mut s, "3i");
    type_ins(&mut s, "x");
    feed(&mut s, "<esc>2..");
    assert_eq!((s.text().as_str(), s.cursor()), ("xxxxxxxabc", Pos::new(0, 4)));

    // `3o` opens a line for every one, each indented like the first.
    let mut s = session("  abc");
    feed(&mut s, "3o");
    host_open_line(&mut s, 0, false, "  ");
    type_ins(&mut s, "foo");
    feed(&mut s, "<esc>");
    assert_eq!((s.text().as_str(), s.cursor()), ("  abc\n  foo\n  foo\n  foo", Pos::new(3, 4)));
}

#[test]
fn dot_after_o_opens_the_line_through_the_host_again() {
    let mut s = session("  abc\nxyz");
    feed(&mut s, "o");
    host_open_line(&mut s, 0, false, "");
    type_ins(&mut s, "foo");
    feed(&mut s, "<esc>j");
    // The repeat goes back to the host as the keys themselves.
    let fx = s.key(Key::Char('.'));
    let run: Vec<&str> = fx
        .commands
        .iter()
        .find_map(|c| match c {
            Command::Execute { keys, count: 1 } => Some(keys.iter().map(String::as_str).collect()),
            _ => None,
        })
        .expect("keys to run");
    assert_eq!(run, ["o", "f", "o", "o", "<esc>"]);
    execute(&mut s, &fx);
    assert_eq!((s.text().as_str(), s.cursor()), ("  abc\nfoo\nxyz\nfoo", Pos::new(3, 2)));
    // With a count, as many lines.
    play(&mut s, "k3.");
    assert_eq!(s.text(), "  abc\nfoo\nxyz\nfoo\nfoo\nfoo\nfoo");
    assert_eq!(s.mode_label(), "normal");
}

#[test]
fn dot_repeats_a_visual_change_over_as_much_text() {
    let cases = [
        // Within a line: as many characters, from the cursor.
        ("wveU0.", "ONE TWO three four 7", (0, 0)),
        ("wvlUw.", "one TWo THree four 7", (0, 8)),
    ];
    for (keys, first, cursor) in cases {
        let mut s = session(LINES);
        feed(&mut s, keys);
        assert_eq!((s.text().lines().next(), s.cursor()), (Some(first), Pos::new(cursor.0, cursor.1)), "{keys}");
    }
    // Over lines: as many lines, linewise; charwise, to the column the first
    // selection ended on.
    let mut s = session("a\nb\nc\nd\ne\nf\ng");
    feed(&mut s, "Vjdj.");
    assert_eq!((s.text().as_str(), s.cursor()), ("c\nf\ng", Pos::new(1, 0)));
    let mut s = session(&format!("{LINES}\nten"));
    feed(&mut s, "wvjd.");
    assert_eq!((s.text().as_str(), s.cursor()), ("one even eight nine 9\nten", Pos::new(0, 4)));
    let mut s = session(&format!("{LINES}\nten"));
    feed(&mut s, "vjlr-j0.");
    assert_eq!(
        s.text(),
        "--------------------\n---------------------\n--x seven eight nine 9\nten"
    );
}

#[test]
fn dot_runs_at_every_cursor() {
    let mut s = session("a\nb\nc");
    feed(&mut s, "A");
    type_ins(&mut s, ";");
    feed(&mut s, "<esc>");
    cursors(&mut s, &[(0, 0), (1, 0), (2, 0)]);
    let fx = feed(&mut s, ".");
    assert_eq!(s.text(), "a;;\nb;\nc;");
    assert_eq!(actives(&fx), [(0, 2), (1, 1), (2, 1)]);
    // Every edit still in the coordinates of the document the host holds.
    let at: Vec<_> = fx.edits.iter().map(|e| (e.start.line, e.start.col, e.text.as_str())).collect();
    assert_eq!(at, [(2, 1, ";"), (1, 1, ";"), (0, 2, ";")]);
}

#[test]
fn an_insert_the_cursor_moved_in_repeats_from_there() {
    let mut s = session("abc def");
    feed(&mut s, "cw");
    type_ins(&mut s, "x");
    s.set_position(0, 2); // the user clicked elsewhere, still inserting
    type_ins(&mut s, "Y");
    feed(&mut s, "<esc>0.");
    // Vim's `.` is then an `i` of what was typed after the move.
    assert_eq!(s.text(), "Yx Ydef");
}

#[test]
fn an_insert_keeps_the_bracket_the_editor_closed_for_it() {
    let mut s = session("f\ng");
    feed(&mut s, "A");
    // The editor closes the `(` typed, and the cursor stays inside.
    s.apply_change(Pos::new(0, 1), Pos::new(0, 1), "()");
    s.set_position(0, 2);
    feed(&mut s, "<esc>");
    assert_eq!(reg(&s, '.'), Some(("()".into(), false)));
    feed(&mut s, "j.");
    assert_eq!((s.text().as_str(), s.cursor()), ("f()\ng()", Pos::new(1, 1)));
}

#[test]
fn q_records_typing_into_a_register() {
    let mut s = session("foo bar\nbaz qux");
    feed(&mut s, "qa");
    assert_eq!(s.recording(), Some('a'));
    feed(&mut s, "ciw");
    type_ins(&mut s, "zap");
    // A backspace typed in insert mode goes in as what it did.
    s.apply_change(Pos::new(0, 2), Pos::new(0, 3), "");
    s.set_position(0, 2);
    feed(&mut s, "<esc>w");
    feed(&mut s, "q");
    assert_eq!(s.recording(), None);
    assert_eq!(reg(&s, 'a'), Some(("ciwza<esc>w".into(), false)));
    // `:reg a` reads the same, and `"ap` puts the keys as text.
    let fx = ex(&mut s, "reg a");
    let detail = fx.commands.iter().find_map(|c| match c {
        Command::ShowList { rows, .. } => Some(rows[0].detail.clone()),
        _ => None,
    });
    assert_eq!(detail.as_deref(), Some("ciwza<esc>w"));
    feed(&mut s, "j0\"aP");
    assert_eq!(s.text().lines().nth(1), Some("ciwza<esc>wbaz qux"));

    // An uppercase name appends; a `<` typed is `<lt>`.
    let mut s = session("x");
    feed(&mut s, "qbxq");
    feed(&mut s, "qBi");
    type_ins(&mut s, "<");
    feed(&mut s, "<esc>q");
    assert_eq!(reg(&s, 'b'), Some(("xi<lt><esc>".into(), false)));
}

#[test]
fn at_runs_a_register_as_keys() {
    let mut s = session("foo bar\nbaz qux\nend");
    feed(&mut s, "qa");
    feed(&mut s, "ciw");
    type_ins(&mut s, "X");
    feed(&mut s, "<esc>jq");
    // `@a` hands the keys to the host to run.
    let fx = s.key(Key::Char('@'));
    assert!(fx.commands.is_empty() && fx.pending == "@");
    let fx = s.key(Key::Char('a'));
    assert!(matches!(&fx.commands[..], [Command::Execute { keys, count: 1 }] if keys.len() == 6));
    execute(&mut s, &fx);
    assert_eq!(s.text(), "X bar\nX qux\nend");
    // `@@` runs it again; a count runs it that many times, and the `j` that
    // fails on the last line stops the rest.
    play(&mut s, "gg0w2@@");
    assert_eq!(s.text(), "X X\nX X\nend");
    play(&mut s, "G@@");
    assert_eq!(s.text(), "X X\nX X\nX");
}

#[test]
fn a_recursive_macro_runs_until_a_motion_fails() {
    // `qaq` empties `a`, so the `@a` recorded inside runs nothing yet.
    let mut s = session("a1\na2\na3\na4");
    play(&mut s, "qaq");
    play(&mut s, "qaA");
    type_ins(&mut s, "Z");
    play(&mut s, "<esc>j@aq");
    assert_eq!(reg(&s, 'a'), Some(("AZ<esc>j@a".into(), false)));
    play(&mut s, "@a");
    assert_eq!(s.text(), "a1Z\na2Z\na3Z\na4Z");
}

#[test]
fn a_failing_key_stops_the_register() {
    // Vim's own list of what fails: each stops the `iZ` after it.
    for keys in ["h", "$l", "j", "b", "$e", "<c-a>", "J", "\"bp", "$2rx", "zq", "dk", ";", "n", "fq", "%", ":foo<cr>", ":s/q/r/<cr>"] {
        let mut s = session("abc");
        assert!(s.set_register('a', &format!("{keys}iZ<esc>"), false));
        play(&mut s, "@a");
        assert_eq!(s.text(), "abc", "{keys}");
    }
    // And what does not.
    for (keys, text) in [("x", "Zbc"), ("X", "Zabc"), ("5j", "abc\nZdef"), ("w", "abZc"), ("$~", "abZC")] {
        let mut s = session(if keys == "5j" { "abc\ndef" } else { "abc" });
        s.set_register('a', &format!("{keys}iZ<esc>"), false);
        play(&mut s, "@a");
        assert_eq!(s.text(), text, "{keys}");
    }
}

#[test]
fn at_colon_repeats_the_last_command_line() {
    // With nothing run yet, `@@` says so.
    let mut s = session("a");
    let fx = feed(&mut s, "@@");
    assert!(fx.failed);
    assert_eq!(fx.message.as_deref(), Some("E748: No previously used register"));
    let mut s = session("a\na\na");
    ex(&mut s, "s/a/b/");
    play(&mut s, "j@:");
    assert_eq!(s.text(), "b\nb\na");
    play(&mut s, "j@@");
    assert_eq!(s.text(), "b\nb\nb");
}

#[test]
fn keys_a_register_runs_are_not_recorded_again() {
    let mut s = session("abc");
    s.set_register('a', "x", false);
    play(&mut s, "qb@aq");
    assert_eq!(reg(&s, 'b'), Some(("@a".into(), false)));
    assert_eq!(s.text(), "bc");
    // A linewise register runs every line with its line break.
    let mut s = session("one\ntwo\nthree");
    s.set_register('c', "x", true);
    play(&mut s, "@c");
    assert_eq!((s.text().as_str(), s.cursor()), ("ne\ntwo\nthree", Pos::new(1, 0)));
}

#[test]
fn a_register_types_in_insert_mode_and_at_every_cursor() {
    let mut s = session("a\nb");
    s.set_register('a', "A!<esc>", false);
    cursors(&mut s, &[(0, 0), (1, 0)]);
    play(&mut s, "@a");
    assert_eq!(s.text(), "a!\nb!");
    assert_eq!(s.mode_label(), "normal");
    // What it typed is the insert's, for `".` and `.`.
    assert_eq!(reg(&s, '.'), Some(("!".into(), false)));
    s.set_position(0, 0);
    feed(&mut s, ".");
    assert_eq!(s.text(), "a!!\nb!");
}
