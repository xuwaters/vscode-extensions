//! End-to-end tests: feed keys, assert buffer text, cursor, and effects.
//! The engine self-applies its edits, so the buffer reflects each keystroke
//! immediately; insert-mode typing is simulated the way the host delivers it
//! (document change + cursor move).

use pretty_assertions::assert_eq;
use vim_engine::buffer::Pos;
use vim_engine::state::{Command, Effects, Session};
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

#[test]
fn open_lines() {
    let mut s = session("one\ntwo");
    feed(&mut s, "o");
    assert_eq!(s.text(), "one\n\ntwo");
    assert_eq!((s.mode_label(), s.cursor()), ("insert", Pos::new(1, 0)));
    feed(&mut s, "<esc>O");
    assert_eq!(s.text(), "one\n\n\ntwo");
    assert_eq!(s.cursor(), Pos::new(1, 0));
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
    let fx = s.set_selection(Pos::new(0, 0), Pos::new(0, 5));
    assert_eq!(fx.mode, "visual");
    feed(&mut s, "d");
    assert_eq!(s.text(), " world");
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

