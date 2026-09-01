//! P2-04 — the `#if` family, the §3.3 constant-expression evaluator, and the
//! inactive regions decision 0003 requires be kept addressable.

use pretty_assertions::assert_eq;

use super::{codes, errors, texts};
use crate::diagnostics::PpCode;
use crate::preprocess_source;

/// The source text of every inactive region, in order.
fn skipped(source: &str) -> Vec<&str> {
    preprocess_source(source)
        .inactive
        .iter()
        .map(|r| &source[r.span.start as usize..r.span.end as usize])
        .collect::<Vec<_>>()
}

/// Whether `#if <expr>` is taken.
fn holds(expression: &str) -> bool {
    let source = format!("#if {expression}\nyes\n#endif\n");
    let pp = preprocess_source(&source);
    assert_eq!(errors(&pp), Vec::<&str>::new(), "for {expression:?}");
    !pp.tokens.is_empty()
}

#[test]
fn ifdef_and_ifndef_follow_the_macro_table() {
    let pp = preprocess_source("#define A\n#ifdef A\nint a;\n#endif\n#ifndef A\nint b;\n#endif\n");
    assert_eq!(texts(&pp), ["int", "a", ";"]);
    let pp = preprocess_source("#ifdef A\nint a;\n#endif\n#ifndef A\nint b;\n#endif\n");
    assert_eq!(texts(&pp), ["int", "b", ";"]);
}

#[test]
fn if_zero_and_if_one() {
    assert_eq!(texts(&preprocess_source("#if 1\nint a;\n#endif\n")), ["int", "a", ";"]);
    assert_eq!(texts(&preprocess_source("#if 0\nint a;\n#endif\n")), Vec::<&str>::new());
}

#[test]
fn defined_in_both_spellings() {
    assert!(holds("defined FOO || 1"));
    let pp = preprocess_source("#define FOO\n#if defined(FOO) && defined FOO\nint a;\n#endif\n");
    assert_eq!(texts(&pp), ["int", "a", ";"]);
    let pp = preprocess_source("#if defined(FOO)\nint a;\n#else\nint b;\n#endif\n");
    assert_eq!(texts(&pp), ["int", "b", ";"]);
}

#[test]
fn defined_sees_the_name_rather_than_its_expansion() {
    // `defined` is resolved before expansion, so `X` here is the name `X`, not
    // whatever `X` stands for.
    let pp = preprocess_source("#define X Y\n#if defined(X)\nint a;\n#endif\n");
    assert_eq!(texts(&pp), ["int", "a", ";"]);
}

#[test]
fn an_undefined_name_is_zero() {
    let pp = preprocess_source("#if NOT_SET\nint a;\n#else\nint b;\n#endif\n");
    assert_eq!(errors(&pp), Vec::<&str>::new());
    assert_eq!(texts(&pp), ["int", "b", ";"]);
}

#[test]
fn macros_are_expanded_inside_the_condition() {
    let pp = preprocess_source("#define N 3\n#define BIG (N > 2)\n#if BIG\nint a;\n#endif\n");
    assert_eq!(texts(&pp), ["int", "a", ";"]);
}

#[test]
fn every_arithmetic_and_logical_operator() {
    assert!(holds("1 + 2 == 3"));
    assert!(holds("7 - 2 * 3 == 1"));
    assert!(holds("7 / 2 == 3"));
    assert!(holds("7 % 2 == 1"));
    assert!(holds("(1 + 2) * 3 == 9"));
    assert!(holds("1 << 4 == 16"));
    assert!(holds("-16 >> 2 == -4"));
    assert!(holds("6 & 3 == 2 || 1"));
    assert!(holds("(6 & 3) == 2"));
    assert!(holds("(6 | 3) == 7"));
    assert!(holds("(6 ^ 3) == 5"));
    assert!(holds("~0 == -1"));
    assert!(holds("!0"));
    assert!(!holds("!1"));
    assert!(holds("-1 < 0"));
    assert!(holds("+1 > 0"));
    assert!(holds("2 >= 2 && 2 <= 2"));
    assert!(holds("1 != 2"));
}

#[test]
fn unary_minus_binds_tighter_than_multiplication() {
    assert!(holds("-2 * 3 == -6"));
}

#[test]
fn literals_in_every_base_and_suffix() {
    assert!(holds("0xFF == 255"));
    assert!(holds("077 == 63"));
    assert!(holds("10u == 10"));
    assert!(holds("0 == 0"));
}

#[test]
fn arithmetic_is_thirty_two_bit_and_wraps() {
    // glslang evaluates `#if` in `int`; 0xFFFFFFFF is -1, not four billion.
    assert!(holds("0xFFFFFFFF == -1"));
    assert!(holds("2147483647 + 1 < 0"));
}

#[test]
fn shifts_out_of_range_are_defined_rather_than_trapping() {
    assert!(holds("1 << 64 == 0"));
    assert!(holds("1 << -1 == 0"));
    assert!(holds("-1 >> 64 == -1"));
    assert!(holds("1 >> 64 == 0"));
}

#[test]
fn division_by_zero_is_reported_and_survives() {
    let pp = preprocess_source("#if 1 / 0\nint a;\n#endif\n");
    assert_eq!(codes(&pp), [PpCode::DivisionByZero]);
    let pp = preprocess_source("#if 1 % 0\nint a;\n#endif\n");
    assert_eq!(codes(&pp), [PpCode::DivisionByZero]);
}

#[test]
fn short_circuiting_hides_the_division_the_condition_never_reaches() {
    // The idiom this exists for: `#if defined(N) && 100 / N > 2` with N unset.
    let pp = preprocess_source("#if 0 && 1 / 0\nint a;\n#endif\n");
    assert_eq!(codes(&pp), Vec::<PpCode>::new());
    let pp = preprocess_source("#if 1 || 1 / 0\nint a;\n#endif\n");
    assert_eq!(codes(&pp), Vec::<PpCode>::new());
    assert_eq!(texts(&pp), ["int", "a", ";"]);
}

#[test]
fn a_broken_expression_is_false_with_one_diagnostic() {
    for source in [
        "#if\nint a;\n#endif\n",
        "#if +\nint a;\n#endif\n",
        "#if (1\nint a;\n#endif\n",
        "#if 1 +\nint a;\n#endif\n",
        "#if ;\nint a;\n#endif\n",
    ] {
        let pp = preprocess_source(source);
        assert_eq!(texts(&pp), Vec::<&str>::new(), "for {source:?}");
        assert!(
            pp.diagnostics.iter().any(|d| d.code == PpCode::BadConditionalExpression),
            "for {source:?}: {:?}",
            codes(&pp)
        );
    }
}

#[test]
fn a_float_in_a_condition_is_rejected_rather_than_rounded() {
    let pp = preprocess_source("#if 1.5\nint a;\n#endif\n");
    assert_eq!(codes(&pp), [PpCode::BadConditionalExpression]);
}

#[test]
fn elif_chains_take_the_first_true_branch_only() {
    let source = "#define N 2\n#if N == 1\nint one;\n#elif N == 2\nint two;\n#elif N == 2\n\
                  int also_two;\n#else\nint other;\n#endif\n";
    assert_eq!(texts(&preprocess_source(source)), ["int", "two", ";"]);
}

#[test]
fn else_runs_when_nothing_else_did() {
    let source = "#if 0\nint a;\n#elif 0\nint b;\n#else\nint c;\n#endif\n";
    assert_eq!(texts(&preprocess_source(source)), ["int", "c", ";"]);
}

#[test]
fn nesting_keeps_the_branches_straight() {
    let source = "#if 1\n#if 0\nint a;\n#else\n#if 1\nint b;\n#else\nint c;\n#endif\n#endif\n\
                  #else\n#if 1\nint d;\n#endif\n#endif\n";
    assert_eq!(texts(&preprocess_source(source)), ["int", "b", ";"]);
}

#[test]
fn a_conditional_inside_a_dead_branch_is_never_evaluated() {
    // The `1 / 0` would be reported if the dead branch were evaluated, and the
    // `#error` would fire.
    let source = "#if 0\n#if 1 / 0\n#error no\n#endif\n#endif\nint a;\n";
    let pp = preprocess_source(source);
    assert_eq!(codes(&pp), Vec::<PpCode>::new());
    assert_eq!(texts(&pp), ["int", "a", ";"]);
}

#[test]
fn a_define_in_a_dead_branch_does_not_land() {
    let source = "#if 0\n#define X 1\n#endif\nint a = X;\n";
    let pp = preprocess_source(source);
    assert!(pp.macros.lookup("X").is_none());
    assert_eq!(texts(&pp), ["int", "a", "=", "X", ";"]);
}

#[test]
fn an_inactive_region_covers_the_skipped_lines_and_nothing_else() {
    let source = "#if 0\nint dead;\n#endif\nint live;\n";
    assert_eq!(skipped(source), ["int dead;\n"]);
    assert_eq!(texts(&preprocess_source(source)), ["int", "live", ";"]);
}

#[test]
fn each_dead_branch_of_a_chain_gets_its_own_region() {
    let source = "#if 0\na;\n#elif 1\nb;\n#elif 0\nc;\n#else\nd;\n#endif\n";
    assert_eq!(skipped(source), ["a;\n", "c;\n", "d;\n"]);
}

#[test]
fn a_dead_branch_inside_a_dead_branch_is_covered_by_the_outer_region_only() {
    let source = "#if 0\nouter;\n#if 1\ninner;\n#endif\nmore;\n#endif\n";
    assert_eq!(skipped(source), ["outer;\n#if 1\ninner;\n#endif\nmore;\n"]);
}

#[test]
fn the_region_names_the_directive_that_switched_it_off() {
    let source = "#if 0\ndead;\n#endif\n";
    let pp = preprocess_source(source);
    let region = &pp.inactive[0];
    assert_eq!(&source[region.directive.start as usize..region.directive.end as usize], "#if 0");
}

#[test]
fn is_inactive_answers_for_an_offset() {
    let source = "#if 0\ndead;\n#endif\nlive;\n";
    let pp = preprocess_source(source);
    let dead = source.find("dead").expect("in the source") as u32;
    let live = source.find("live").expect("in the source") as u32;
    assert!(pp.is_inactive(dead));
    assert!(!pp.is_inactive(live));
}

#[test]
fn an_empty_dead_branch_records_no_region() {
    let pp = preprocess_source("#if 0\n#endif\nint a;\n");
    assert!(pp.inactive.is_empty());
}

#[test]
fn unmatched_conditionals_are_diagnosed_rather_than_panicking() {
    assert_eq!(codes(&preprocess_source("#endif\n")), [PpCode::UnmatchedConditional]);
    assert_eq!(codes(&preprocess_source("#else\n")), [PpCode::UnmatchedConditional]);
    assert_eq!(codes(&preprocess_source("#elif 1\n")), [PpCode::UnmatchedConditional]);
}

#[test]
fn an_unterminated_conditional_is_diagnosed_at_the_directive_that_opened_it() {
    let source = "#if 1\nint a;\n";
    let pp = preprocess_source(source);
    assert_eq!(codes(&pp), [PpCode::UnterminatedConditional]);
    let span = pp.diagnostics[0].span;
    assert_eq!(&source[span.start as usize..span.end as usize], "#if 1");
    // The live branch is still analysed; an unclosed `#if` is what a
    // half-typed file looks like.
    assert_eq!(texts(&pp), ["int", "a", ";"]);
}

#[test]
fn an_unterminated_dead_conditional_leaves_a_region_reaching_the_end_of_file() {
    let source = "#if 0\nint a;\n";
    assert_eq!(skipped(source), ["int a;\n"]);
}

#[test]
fn two_elses_are_diagnosed() {
    let pp = preprocess_source("#if 0\na;\n#else\nb;\n#else\nc;\n#endif\n");
    assert!(codes(&pp).contains(&PpCode::MisplacedElse));
    let pp = preprocess_source("#if 0\na;\n#else\nb;\n#elif 1\nc;\n#endif\n");
    assert!(codes(&pp).contains(&PpCode::MisplacedElse));
}

#[test]
fn ifdef_without_a_name_is_diagnosed() {
    assert_eq!(codes(&preprocess_source("#ifdef\n#endif\n")), [PpCode::MissingMacroName]);
    assert_eq!(codes(&preprocess_source("#ifndef 1\n#endif\n")), [PpCode::MissingMacroName]);
}

#[test]
fn a_deep_nest_neither_overflows_nor_loses_track() {
    let depth = 200;
    let mut source = String::new();
    for _ in 0..depth {
        source.push_str("#if 1\n");
    }
    source.push_str("int a;\n");
    for _ in 0..depth {
        source.push_str("#endif\n");
    }
    let pp = preprocess_source(&source);
    assert_eq!(errors(&pp), Vec::<&str>::new());
    assert_eq!(texts(&pp), ["int", "a", ";"]);
}
