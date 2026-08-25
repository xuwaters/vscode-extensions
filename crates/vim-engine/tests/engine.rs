//! End-to-end tests: feed keys, assert buffer text, cursor, and effects.
//! The engine self-applies its edits, so the buffer reflects each keystroke
//! immediately; insert-mode typing is simulated the way the host delivers it
//! (document change + cursor move).

use pretty_assertions::assert_eq;
use vim_engine::buffer::Pos;
use vim_engine::state::{Command, EasyUi, Effects, SearchUi, Session};
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
        [Command::IndentLines { start_line: 0, end_line: 0, dedent: false }]
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
    assert_eq!((s.mode_label(), s.text().as_str()), ("insert", ""));
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
    let fx = feed(&mut s, ":w<cr>");
    assert_eq!(fx.message.as_deref(), Some("not an editor command: w"));
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
    // One register for all cursors, so p pastes the same text everywhere.
    let mut s = session("ab\ncd");
    cursors(&mut s, &[(0, 0), (1, 0)]);
    feed(&mut s, "ylp");
    assert_eq!(s.text(), "aab\ncad");
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
            Command::IndentLines { start_line: 2, end_line: 2, dedent: false },
            Command::IndentLines { start_line: 0, end_line: 0, dedent: false },
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
