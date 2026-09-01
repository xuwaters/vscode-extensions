//! P2-05 — expansion, and the provenance rules of decision 0003.
//!
//! Every fixture here asserts *spans*, via [`super::attributed`], which reads
//! the original source back through each token's span. A rule inversion —
//! body tokens keeping the `#define`'s span, argument tokens losing theirs —
//! changes the strings these tests compare, so text-only assertions would not
//! catch it.

use pretty_assertions::assert_eq;

use super::{attributed, codes, errors, texts};
use crate::diagnostics::PpCode;
use crate::preprocessor::Origin;
use crate::preprocess_source;

/// `(text, the source text the token points at, origin)` for every live token.
fn trace(source: &str) -> Vec<(String, String, Origin)> {
    let pp = preprocess_source(source);
    assert_eq!(errors(&pp), Vec::<&str>::new(), "for {source:?}");
    pp.tokens
        .iter()
        .map(|t| {
            (
                t.text.clone(),
                source[t.span.start as usize..t.span.end as usize].to_string(),
                t.origin,
            )
        })
        .collect()
}

#[test]
fn an_object_like_body_token_points_at_the_invocation() {
    let source = "#define PI 3.14\nfloat x = PI;\n";
    assert_eq!(trace(source), [
        ("float".into(), "float".into(), Origin::Source),
        ("x".into(), "x".into(), Origin::Source),
        ("=".into(), "=".into(), Origin::Source),
        // Not `3.14` in the `#define` — the `PI` the user is looking at.
        ("3.14".into(), "PI".into(), Origin::MacroBody),
        (";".into(), ";".into(), Origin::Source),
    ]);
}

#[test]
fn every_token_of_a_multi_token_body_points_at_the_same_invocation() {
    let source = "#define V vec3(1.0)\nV;\n";
    let pp = preprocess_source(source);
    assert_eq!(texts(&pp), ["vec3", "(", "1.0", ")", ";"]);
    assert_eq!(attributed(&pp, source), ["V", "V", "V", "V", ";"]);
}

#[test]
fn argument_tokens_keep_the_span_they_were_written_at() {
    let source = "#define ID(x) x\nint a = ID(myVar);\n";
    assert_eq!(trace(source), [
        ("int".into(), "int".into(), Origin::Source),
        ("a".into(), "a".into(), Origin::Source),
        ("=".into(), "=".into(), Origin::Source),
        // The whole point: `myVar` is still findable at the bytes the user
        // typed, so references and rename work through the macro.
        ("myVar".into(), "myVar".into(), Origin::MacroArg),
        (";".into(), ";".into(), Origin::Source),
    ]);
}

#[test]
fn body_and_argument_tokens_are_mixed_correctly_in_one_expansion() {
    let source = "#define SCALE(v) ((v) * 2.0)\nfloat y = SCALE(input);\n";
    let pp = preprocess_source(source);
    assert_eq!(texts(&pp), ["float", "y", "=", "(", "(", "input", ")", "*", "2.0", ")", ";"]);
    assert_eq!(attributed(&pp, source), [
        "float",
        "y",
        "=",
        // Body tokens land on the whole invocation, name through `)`.
        "SCALE(input)",
        "SCALE(input)",
        "input",
        "SCALE(input)",
        "SCALE(input)",
        "SCALE(input)",
        "SCALE(input)",
        ";",
    ]);
}

#[test]
fn a_nested_invocation_keeps_the_inner_arguments_addressable() {
    let source = "#define ID(x) x\nint a = ID(ID(deep));\n";
    let pp = preprocess_source(source);
    assert_eq!(texts(&pp), ["int", "a", "=", "deep", ";"]);
    assert_eq!(attributed(&pp, source), ["int", "a", "=", "deep", ";"]);
    assert_eq!(pp.tokens[3].origin, Origin::MacroArg);
}

#[test]
fn a_macro_used_inside_an_argument_is_expanded_before_substitution() {
    let source = "#define TWO 2\n#define DOUBLE(x) ((x) + (x))\nint a = DOUBLE(TWO);\n";
    let pp = preprocess_source(source);
    assert_eq!(texts(&pp), ["int", "a", "=", "(", "(", "2", ")", "+", "(", "2", ")", ")", ";"]);
    // The `2`s came from `TWO`, which is where a hover on them should land.
    assert_eq!(attributed(&pp, source)[5], "TWO");
    assert_eq!(pp.tokens[5].origin, Origin::MacroBody);
}

#[test]
fn origins_say_whether_a_span_is_text_the_user_wrote() {
    let source = "#define ID(x) x + 1\nID(v);\n";
    let pp = preprocess_source(source);
    let written: Vec<_> = pp.tokens.iter().map(|t| t.origin.is_written()).collect();
    assert_eq!(texts(&pp), ["v", "+", "1", ";"]);
    assert_eq!(written, [true, false, false, true]);
}

#[test]
fn an_object_like_macro_rescans_into_another_macro() {
    let source = "#define A B\n#define B 7\nint a = A;\n";
    let pp = preprocess_source(source);
    assert_eq!(texts(&pp), ["int", "a", "=", "7", ";"]);
    // Two hops of body substitution still land on the one thing the user wrote.
    assert_eq!(attributed(&pp, source)[3], "A");
}

#[test]
fn a_macro_may_expand_into_a_call_to_another_macro() {
    let source = "#define ID(x) x\n#define WRAP ID(9)\nint a = WRAP;\n";
    let pp = preprocess_source(source);
    assert_eq!(texts(&pp), ["int", "a", "=", "9", ";"]);
    assert_eq!(attributed(&pp, source)[3], "WRAP");
}

#[test]
fn a_macro_name_may_arrive_from_one_expansion_and_its_arguments_from_the_source() {
    // The case a naive recursive expander gets wrong: `CALL` produces the name
    // `ID`, and the `(1)` that completes the call is the next source token.
    let source = "#define ID(x) x\n#define CALL ID\nint a = CALL(1);\n";
    let pp = preprocess_source(source);
    assert_eq!(texts(&pp), ["int", "a", "=", "1", ";"]);
}

#[test]
fn a_self_referential_macro_expands_once_and_stops() {
    let source = "#define X X\nint a = X;\n";
    let pp = preprocess_source(source);
    assert_eq!(errors(&pp), Vec::<&str>::new());
    assert_eq!(texts(&pp), ["int", "a", "=", "X", ";"]);
    assert_eq!(pp.tokens[3].origin, Origin::MacroBody);
}

#[test]
fn a_macro_that_mentions_itself_in_a_larger_body_stops_too() {
    let pp = preprocess_source("#define X (1 + X)\nint a = X;\n");
    assert_eq!(errors(&pp), Vec::<&str>::new());
    assert_eq!(texts(&pp), ["int", "a", "=", "(", "1", "+", "X", ")", ";"]);
}

#[test]
fn mutually_recursive_macros_terminate() {
    let pp = preprocess_source("#define A B\n#define B A\nint a = A;\n");
    assert_eq!(errors(&pp), Vec::<&str>::new());
    assert_eq!(texts(&pp), ["int", "a", "=", "A", ";"]);
}

#[test]
fn a_self_referential_function_like_macro_terminates() {
    let pp = preprocess_source("#define F(x) F(x)\nint a = F(1);\n");
    assert_eq!(errors(&pp), Vec::<&str>::new());
    assert_eq!(texts(&pp), ["int", "a", "=", "F", "(", "1", ")", ";"]);
}

#[test]
fn a_function_like_macro_that_is_not_called_stays_a_name() {
    let pp = preprocess_source("#define F(x) x\nint a = F;\nint b = F + 1;\n");
    assert_eq!(errors(&pp), Vec::<&str>::new());
    assert_eq!(texts(&pp), ["int", "a", "=", "F", ";", "int", "b", "=", "F", "+", "1", ";"]);
}

#[test]
fn an_invocation_may_span_several_lines() {
    let source = "#define ADD(a, b) ((a) + (b))\nint x = ADD(1,\n            2);\n";
    let pp = preprocess_source(source);
    assert_eq!(errors(&pp), Vec::<&str>::new());
    assert_eq!(texts(&pp), ["int", "x", "=", "(", "(", "1", ")", "+", "(", "2", ")", ")", ";"]);
    assert_eq!(attributed(&pp, source)[5], "1");
    assert_eq!(attributed(&pp, source)[9], "2");
}

#[test]
fn commas_inside_parentheses_do_not_split_an_argument() {
    let pp = preprocess_source("#define ID(x) x\nint a = ID(vec2(1, 2));\n");
    assert_eq!(errors(&pp), Vec::<&str>::new());
    assert_eq!(texts(&pp), ["int", "a", "=", "vec2", "(", "1", ",", "2", ")", ";"]);
}

#[test]
fn an_empty_argument_substitutes_nothing() {
    let pp = preprocess_source("#define ID(x) [x]\nint a = ID();\n");
    assert_eq!(errors(&pp), Vec::<&str>::new());
    assert_eq!(texts(&pp), ["int", "a", "=", "[", "]", ";"]);
}

#[test]
fn calling_a_zero_parameter_macro_with_empty_parens_is_zero_arguments() {
    let pp = preprocess_source("#define NOW() 5\nint a = NOW();\n");
    assert_eq!(errors(&pp), Vec::<&str>::new());
    assert_eq!(texts(&pp), ["int", "a", "=", "5", ";"]);
}

#[test]
fn the_wrong_number_of_arguments_is_diagnosed_rather_than_panicking() {
    let pp = preprocess_source("#define ADD(a, b) a + b\nint x = ADD(1);\n");
    assert_eq!(codes(&pp), [PpCode::MacroArgumentCount]);
    // …and the stream is still usable: the missing argument is simply empty.
    assert_eq!(texts(&pp), ["int", "x", "=", "1", "+", ";"]);

    let pp = preprocess_source("#define ID(a) a\nint x = ID(1, 2);\n");
    assert_eq!(codes(&pp), [PpCode::MacroArgumentCount]);
}

#[test]
fn an_unterminated_invocation_is_diagnosed_and_the_tokens_survive() {
    let pp = preprocess_source("#define ADD(a, b) a + b\nint x = ADD(1, 2;\n");
    assert_eq!(codes(&pp), [PpCode::UnterminatedMacroInvocation]);
    assert_eq!(texts(&pp), ["int", "x", "=", "ADD", "(", "1", ",", "2", ";"]);
}

#[test]
fn token_pasting_joins_two_tokens_into_one() {
    let source = "#define CAT(a, b) a##b\nint CAT(my, Var) = 1;\n";
    let pp = preprocess_source(source);
    assert_eq!(errors(&pp), Vec::<&str>::new());
    assert_eq!(texts(&pp), ["int", "myVar", "=", "1", ";"]);
    // A pasted token exists in no source slice, so it is attributed to the
    // invocation, like a body token.
    assert_eq!(attributed(&pp, source)[1], "CAT(my, Var)");
    assert_eq!(pp.tokens[1].origin, Origin::MacroBody);
}

#[test]
fn pasting_takes_the_argument_unexpanded() {
    // `a##b` sees the parameters as written: `TWO` is pasted as the name it is,
    // not as the `2` it stands for.
    let pp = preprocess_source("#define TWO 2\n#define CAT(a, b) a##b\nint x = CAT(TWO, x);\n");
    assert_eq!(texts(&pp), ["int", "x", "=", "TWOx", ";"]);
}

#[test]
fn the_result_of_a_paste_is_rescanned() {
    // …but once pasted, the result is an ordinary token again, and a macro name
    // it happens to spell does expand.
    let pp = preprocess_source("#define TWO 2\n#define CAT(a, b) a##b\nint x = CAT(TW, O);\n");
    assert_eq!(errors(&pp), Vec::<&str>::new());
    assert_eq!(texts(&pp), ["int", "x", "=", "2", ";"]);
}

#[test]
fn pasting_numbers_makes_a_number() {
    let pp = preprocess_source("#define CAT(a, b) a##b\nint x = CAT(1, 2);\n");
    assert_eq!(errors(&pp), Vec::<&str>::new());
    assert_eq!(texts(&pp), ["int", "x", "=", "12", ";"]);
}

#[test]
fn a_paste_that_makes_no_single_token_is_reported_and_both_survive() {
    let pp = preprocess_source("#define CAT(a, b) a##b\nint x = CAT(1, +);\n");
    assert_eq!(codes(&pp), [PpCode::BadTokenPaste]);
    assert_eq!(texts(&pp), ["int", "x", "=", "1", "+", ";"]);
}

#[test]
fn a_paste_at_the_edge_of_a_body_is_reported_rather_than_panicking() {
    assert!(codes(&preprocess_source("#define A ## 1\nint x = A;\n"))
        .contains(&PpCode::BadTokenPaste));
    assert!(codes(&preprocess_source("#define A 1 ##\nint x = A;\n"))
        .contains(&PpCode::BadTokenPaste));
}

#[test]
fn stringify_quotes_the_argument_as_written() {
    let source = "#define STR(x) #x\n#pragma STR(a + b)\n";
    let pp = preprocess_source(source);
    assert_eq!(errors(&pp), Vec::<&str>::new());
    // The pragma text is raw source, so read the macro's effect through a use
    // in code instead.
    let pp = preprocess_source("#define STR(x) #x\nconst char* s = STR(a + b);\n");
    assert_eq!(texts(&pp).last(), Some(&";"));
    assert!(texts(&pp).contains(&"\"a + b\""));
}

#[test]
fn a_stringify_of_something_that_is_not_a_parameter_is_reported() {
    let pp = preprocess_source("#define BAD(x) #y\nint a = BAD(1);\n");
    assert!(codes(&pp).contains(&PpCode::BadStringify));
}

#[test]
fn line_inside_a_macro_body_reports_the_invocation_line() {
    // `__LINE__` in a body takes its value from where the macro was used, not
    // from where it was defined — the same rule the spans follow.
    let source = "#define HERE __LINE__\nint a = 0;\nint b = HERE;\n";
    let pp = preprocess_source(source);
    assert_eq!(texts(&pp), ["int", "a", "=", "0", ";", "int", "b", "=", "3", ";"]);
    assert_eq!(attributed(&pp, source)[8], "HERE");
}

#[test]
fn expansion_of_a_pathological_source_terminates_with_a_diagnostic() {
    // The classic exponential blow-up. It must stop, report, and still return
    // a token stream rather than hanging or overflowing the stack.
    let mut source = String::from("#define A0 x\n");
    for i in 1..24 {
        source.push_str(&format!("#define A{i} A{} A{}\n", i - 1, i - 1));
    }
    source.push_str("int v = A23;\n");
    let pp = preprocess_source(&source);
    assert!(pp.diagnostics.iter().any(|d| d.code == PpCode::ExpansionLimit));
    assert!(!pp.tokens.is_empty());
}

#[test]
fn a_deeply_nested_argument_does_not_overflow_the_stack() {
    let mut source = String::from("#define ID(x) x\nint v = ");
    let depth = 300;
    for _ in 0..depth {
        source.push_str("ID(");
    }
    source.push('1');
    for _ in 0..depth {
        source.push(')');
    }
    source.push_str(";\n");
    let pp = preprocess_source(&source);
    assert_eq!(texts(&pp).first(), Some(&"int"));
    assert_eq!(texts(&pp).last(), Some(&";"));
}
