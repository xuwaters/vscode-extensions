//! P2-01 — the token layer.
//!
//! The invariant every one of these leans on is losslessness: concatenating the
//! spans of the token stream must reproduce the source exactly, for *any*
//! input. [`covers_every_byte`] asserts it over a pile of pathological sources
//! so the individual fixtures can concentrate on classification.

use pretty_assertions::assert_eq;

use crate::lexer::{Punct, Token, TokenKind, tokenize};

/// Kinds and spellings of the non-trivia tokens.
fn significant(source: &str) -> Vec<(TokenKind, String)> {
    tokenize(source)
        .iter()
        .filter(|t| !t.kind.is_trivia())
        .map(|t| (t.kind, t.text(source).into_owned()))
        .collect()
}

fn kinds(source: &str) -> Vec<TokenKind> {
    significant(source).into_iter().map(|(k, _)| k).collect()
}

/// The lossless invariant: spans partition the source, in order, with no gap
/// and no overlap.
fn assert_lossless(source: &str, tokens: &[Token]) {
    let mut at = 0u32;
    for token in tokens {
        assert_eq!(token.span.start, at, "gap or overlap before {token:?} in {source:?}");
        assert!(token.span.end > token.span.start, "empty token {token:?} in {source:?}");
        at = token.span.end;
    }
    assert_eq!(at as usize, source.len(), "trailing bytes unclaimed in {source:?}");
}

#[test]
fn covers_every_byte() {
    let sources = [
        "",
        "\n",
        "  \t \r\n",
        "void main() {}",
        "/* unterminated",
        "// trailing comment",
        "#version 460 core\nlayout(location=0) in vec3 p;",
        "1.0e+5f 0xFFu 077 .5 1. 1e 0x 3lf",
        "$ @ ` \\ '",
        "a\\\nb",
        "\"unterminated",
        "\"a\\\"b\"",
        "float \u{3b1} = 1.0; // \u{1f980} unicode in a comment",
        "#define X(a,b) a##b\nX(fo,o)",
        "<<= >>= ^^ ## <<>>",
        "\r\n\r\n",
        "0",
    ];
    for source in sources {
        let tokens = tokenize(source);
        assert_lossless(source, &tokens);
    }
}

#[test]
fn identifiers_and_keywords_are_one_kind() {
    // Which words are reserved depends on the version, which the lexer does not
    // know, so `discard` and `myVar` are both just words here.
    assert_eq!(
        significant("discard myVar _1 gl_Position"),
        [
            (TokenKind::Ident, "discard".to_string()),
            (TokenKind::Ident, "myVar".to_string()),
            (TokenKind::Ident, "_1".to_string()),
            (TokenKind::Ident, "gl_Position".to_string()),
        ]
    );
}

#[test]
fn integer_literals_in_every_base_and_suffix() {
    assert_eq!(
        kinds("0 7 42 0x1F 0XdeadBEEF 0777 12u 12U 5l 5UL"),
        [TokenKind::Int; 10]
    );
}

#[test]
fn float_literals_in_every_form() {
    assert_eq!(
        kinds("1.0 .5 1. 1e5 1E-5 1.0e+5 1.5f 1.5F 2.0lf 3.0LF 1e5f"),
        [TokenKind::Float; 11]
    );
}

#[test]
fn a_bare_f_suffix_makes_an_integer_a_float() {
    assert_eq!(kinds("1f"), [TokenKind::Float]);
    assert_eq!(kinds("1lf"), [TokenKind::Float]);
    assert_eq!(kinds("1u"), [TokenKind::Int]);
}

#[test]
fn a_dot_after_a_number_is_part_of_it_but_a_dot_alone_is_not() {
    assert_eq!(
        significant("v.x"),
        [
            (TokenKind::Ident, "v".to_string()),
            (TokenKind::Punct(Punct::Dot), ".".to_string()),
            (TokenKind::Ident, "x".to_string()),
        ]
    );
    assert_eq!(significant("1.0"), [(TokenKind::Float, "1.0".to_string())]);
}

#[test]
fn a_trailing_e_is_not_an_exponent() {
    // `1e` has no digits after the `e`, so the `e` is a suffix, not an
    // exponent — and the whole thing is still one token so the parser can
    // complain about it as a unit.
    assert_eq!(significant("1e"), [(TokenKind::Int, "1e".to_string())]);
    assert_eq!(significant("1e+"), [
        (TokenKind::Int, "1e".to_string()),
        (TokenKind::Punct(Punct::Plus), "+".to_string()),
    ]);
}

#[test]
fn operators_are_lexed_at_max_munch() {
    assert_eq!(
        kinds("<<= >>= << >> <= >= == != && ^^ || ++ -- += ## #"),
        [
            TokenKind::Punct(Punct::ShlEq),
            TokenKind::Punct(Punct::ShrEq),
            TokenKind::Punct(Punct::Shl),
            TokenKind::Punct(Punct::Shr),
            TokenKind::Punct(Punct::Le),
            TokenKind::Punct(Punct::Ge),
            TokenKind::Punct(Punct::EqEq),
            TokenKind::Punct(Punct::Ne),
            TokenKind::Punct(Punct::AndAnd),
            TokenKind::Punct(Punct::XorXor),
            TokenKind::Punct(Punct::OrOr),
            TokenKind::Punct(Punct::PlusPlus),
            TokenKind::Punct(Punct::MinusMinus),
            TokenKind::Punct(Punct::PlusEq),
            TokenKind::Punct(Punct::HashHash),
            TokenKind::Punct(Punct::Hash),
        ]
    );
}

#[test]
fn every_punct_spelling_round_trips() {
    for punct in [
        Punct::LParen, Punct::RParen, Punct::LBracket, Punct::RBracket, Punct::LBrace,
        Punct::RBrace, Punct::Dot, Punct::Comma, Punct::Colon, Punct::Semi, Punct::Question,
        Punct::Plus, Punct::Minus, Punct::Star, Punct::Slash, Punct::Percent, Punct::Tilde,
        Punct::Bang, Punct::Amp, Punct::Caret, Punct::Pipe, Punct::Lt, Punct::Gt, Punct::Le,
        Punct::Ge, Punct::EqEq, Punct::Ne, Punct::Shl, Punct::Shr, Punct::AndAnd, Punct::XorXor,
        Punct::OrOr, Punct::PlusPlus, Punct::MinusMinus, Punct::Eq, Punct::PlusEq, Punct::MinusEq,
        Punct::StarEq, Punct::SlashEq, Punct::PercentEq, Punct::ShlEq, Punct::ShrEq, Punct::AmpEq,
        Punct::CaretEq, Punct::PipeEq, Punct::Hash, Punct::HashHash,
    ] {
        let text = punct.as_str();
        assert_eq!(
            significant(text),
            [(TokenKind::Punct(punct), text.to_string())],
            "{text:?} did not lex back to itself"
        );
    }
}

#[test]
fn comments_are_trivia_and_survive_unterminated() {
    let source = "a // one\nb /* two";
    let tokens = tokenize(source);
    assert_lossless(source, &tokens);
    let comments: Vec<_> = tokens
        .iter()
        .filter(|t| t.kind == TokenKind::Comment)
        .map(|t| t.text(source).into_owned())
        .collect();
    assert_eq!(comments, ["// one", "/* two"]);
}

#[test]
fn block_comments_do_not_nest_in_glsl() {
    // Unlike WGSL. The first `*/` closes it, so `b` is code.
    let source = "/* /* */ b";
    assert_eq!(significant(source), [(TokenKind::Ident, "b".to_string())]);
}

#[test]
fn whitespace_and_newlines_are_separate_trivia() {
    let source = "a \n b";
    let kinds: Vec<_> = tokenize(source).iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        [
            TokenKind::Ident,
            TokenKind::Space,
            TokenKind::Newline,
            TokenKind::Space,
            TokenKind::Ident,
        ]
    );
}

#[test]
fn crlf_is_one_newline_token() {
    let source = "a\r\nb";
    let tokens = tokenize(source);
    assert_lossless(source, &tokens);
    assert_eq!(tokens[1].kind, TokenKind::Newline);
    assert_eq!(tokens[1].span.len(), 2);
}

#[test]
fn line_continuation_between_tokens_is_its_own_trivia() {
    // `a` cannot continue into `+`, so the splice ends up between two tokens
    // and becomes trivia in its own right.
    let source = "a\\\n+b";
    let tokens = tokenize(source);
    assert_lossless(source, &tokens);
    assert_eq!(tokens[1].kind, TokenKind::LineContinuation);
    assert_eq!(
        significant(source),
        [
            (TokenKind::Ident, "a".to_string()),
            (TokenKind::Punct(Punct::Plus), "+".to_string()),
            (TokenKind::Ident, "b".to_string()),
        ]
    );
}

#[test]
fn a_continuation_between_two_word_characters_joins_them() {
    // The C rule, and the reason the splice cannot be handled as whitespace:
    // there is no token boundary here at all.
    assert_eq!(significant("a\\\nb"), [(TokenKind::Ident, "ab".to_string())]);
    assert_eq!(significant("12\\\n34"), [(TokenKind::Int, "1234".to_string())]);
}

#[test]
fn line_continuation_inside_a_token_splices_it() {
    // The case the whole `spliced` flag exists for: the name is `FOO`, spelled
    // across two physical lines.
    let source = "FO\\\nO";
    let tokens = tokenize(source);
    assert_lossless(source, &tokens);
    assert_eq!(tokens.len(), 1);
    assert!(tokens[0].spliced);
    assert_eq!(tokens[0].text(source), "FOO");
    // The span still covers every byte, continuation included, so highlighting
    // and rename see the whole thing.
    assert_eq!(tokens[0].span.len() as usize, source.len());
}

#[test]
fn several_line_continuations_in_a_row_splice_once() {
    let source = "FO\\\n\\\nO";
    let tokens = tokenize(source);
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].text(source), "FOO");
}

#[test]
fn a_continuation_extends_a_line_comment() {
    // glslang's scanner hides the splice from the comment scanner, so the
    // comment swallows the next line too.
    let source = "// a \\\nstill comment\ncode";
    assert_eq!(significant(source), [(TokenKind::Ident, "code".to_string())]);
}

#[test]
fn a_backslash_that_is_not_a_continuation_is_unknown() {
    let source = "a \\ b";
    let tokens = tokenize(source);
    assert_lossless(source, &tokens);
    assert_eq!(kinds(source), [TokenKind::Ident, TokenKind::Unknown, TokenKind::Ident]);
}

#[test]
fn stray_bytes_become_unknown_rather_than_stopping_the_lexer() {
    assert_eq!(kinds("a $ b @ c"), [
        TokenKind::Ident,
        TokenKind::Unknown,
        TokenKind::Ident,
        TokenKind::Unknown,
        TokenKind::Ident,
    ]);
}

#[test]
fn strings_stop_at_the_end_of_their_line() {
    let source = "\"abc\ndef";
    let tokens = tokenize(source);
    assert_lossless(source, &tokens);
    assert_eq!(tokens[0].kind, TokenKind::Str);
    assert_eq!(tokens[0].text(source), "\"abc");
}

#[test]
fn at_line_start_marks_the_hash_of_a_directive_only() {
    let source = "#version 460\nint a = b # c;\n  #define X 1";
    let hashes: Vec<_> = tokenize(source)
        .iter()
        .filter(|t| t.kind == TokenKind::Punct(Punct::Hash))
        .map(|t| t.at_line_start)
        .collect();
    assert_eq!(hashes, [true, false, true]);
}

#[test]
fn a_hash_after_a_continuation_is_still_at_line_start() {
    let source = "\\\n#define X 1";
    let hash = tokenize(source)
        .into_iter()
        .find(|t| t.kind == TokenKind::Punct(Punct::Hash))
        .expect("a hash");
    assert!(hash.at_line_start);
}

#[test]
fn lines_count_physical_lines_including_spliced_ones() {
    let source = "a\nb\\\n+c\nd";
    let lines: Vec<_> = tokenize(source)
        .iter()
        .filter(|t| t.kind == TokenKind::Ident)
        .map(|t| t.line)
        .collect();
    assert_eq!(lines, [0, 1, 2, 3]);
}

#[test]
fn a_spliced_token_is_reported_on_the_line_it_starts_on() {
    let source = "a\nFO\\\nO\nb";
    let spliced = tokenize(source)
        .into_iter()
        .find(|t| t.spliced)
        .expect("the identifier is spliced");
    assert_eq!(spliced.text(source), "FOO");
    assert_eq!(spliced.line, 1);
    // …and the token after it is back on the physical line it occupies.
    let last = tokenize(source)
        .into_iter()
        .rfind(|t| t.kind == TokenKind::Ident)
        .expect("an identifier");
    assert_eq!(last.line, 3);
}

#[test]
fn multibyte_text_never_splits_a_scalar() {
    // An identifier cannot hold a non-ASCII byte in GLSL, so this is a run of
    // Unknown tokens — but each must be one whole char, not one byte.
    let source = "\u{3b1}\u{1f980}";
    let tokens = tokenize(source);
    assert_lossless(source, &tokens);
    assert_eq!(tokens.len(), 2);
    assert_eq!(tokens[0].span.len(), 2);
    assert_eq!(tokens[1].span.len(), 4);
}
