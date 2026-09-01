//! P4-09 — the diagnostics catalogue: one seeded fixture per code.
//!
//! The table below is the same one design/diagnostics.md prints, and the tests
//! hold it to three things:
//!
//! 1. **Every code has a fixture that produces exactly it** — the whole file
//!    reports that code and nothing else. A code nothing can emit is a code
//!    nobody can act on.
//! 2. **Every fixture's valid twin is silent.** Each row carries the same
//!    shader written correctly, and it must produce no error at all.
//! 3. **The codes are the ones the range promises**: `GLSL0200` onward,
//!    contiguous, unique, and never renumbered.

use analyzer_core::diagnostics::{DiagnosticCode, Severity};

use crate::SemanticCode;
use crate::tests::{analyze, codes, messages};

/// One catalogue row: the code, a source that produces it, and the same shader
/// written the way it should be.
struct Row {
    code: SemanticCode,
    broken: &'static str,
    fixed: &'static str,
}

const CATALOGUE: &[Row] = &[
    Row {
        code: SemanticCode::UnknownIdentifier,
        broken: "void main() { float f = missing; }\n",
        fixed: "void main() { float missing = 1.0; float f = missing; }\n",
    },
    Row {
        code: SemanticCode::UnknownFunction,
        broken: "void main() { missing(1.0); }\n",
        fixed: "void missing(float x) { }\nvoid main() { missing(1.0); }\n",
    },
    Row {
        code: SemanticCode::UnknownType,
        broken: "void main() { flaot f = 1.0; }\n",
        fixed: "void main() { float f = 1.0; }\n",
    },
    Row {
        code: SemanticCode::Redeclaration,
        broken: "float x;\nfloat x;\n",
        fixed: "float x;\nfloat y;\n",
    },
    Row {
        code: SemanticCode::UnknownMember,
        broken: "struct S { int a; };\nS s;\nvoid main() { int b = s.b; }\n",
        fixed: "struct S { int a; };\nS s;\nvoid main() { int b = s.a; }\n",
    },
    Row {
        code: SemanticCode::BadSwizzle,
        broken: "vec2 v;\nvoid main() { float f = v.z; }\n",
        fixed: "vec2 v;\nvoid main() { float f = v.y; }\n",
    },
    Row {
        code: SemanticCode::NotAStruct,
        broken: "float f;\nvoid main() { float g = f.member; }\n",
        fixed: "struct S { float member; };\nS f;\nvoid main() { float g = f.member; }\n",
    },
    Row {
        code: SemanticCode::NotIndexable,
        broken: "float f;\nvoid main() { float g = f[0]; }\n",
        fixed: "float f[2];\nvoid main() { float g = f[0]; }\n",
    },
    Row {
        code: SemanticCode::IndexOutOfRange,
        broken: "vec3 v;\nvoid main() { float f = v[3]; }\n",
        fixed: "vec3 v;\nvoid main() { float f = v[2]; }\n",
    },
    Row {
        code: SemanticCode::BadConstructor,
        broken: "void main() { vec3 v = vec3(1.0, 2.0); }\n",
        fixed: "void main() { vec3 v = vec3(1.0, 2.0, 3.0); }\n",
    },
    Row {
        code: SemanticCode::NoMatchingOverload,
        broken: "void main() { float f = sin(true); }\n",
        fixed: "void main() { float f = sin(1.0); }\n",
    },
    Row {
        code: SemanticCode::AmbiguousCall,
        broken: "int g(float x, int y);\nfloat g(int x, float y);\n\
                 void main() { g(1, 1); }\n",
        fixed: "int g(float x, int y);\nfloat g(int x, float y);\n\
                void main() { g(1.0, 1); }\n",
    },
    Row {
        code: SemanticCode::ArgumentCount,
        broken: "float f(float a, float b) { return a; }\nvoid main() { f(1.0); }\n",
        fixed: "float f(float a, float b) { return a; }\nvoid main() { f(1.0, 2.0); }\n",
    },
    Row {
        code: SemanticCode::ArgumentType,
        broken: "float f(vec3 v) { return v.x; }\nvoid main() { f(1.0); }\n",
        fixed: "float f(vec3 v) { return v.x; }\nvoid main() { f(vec3(1.0)); }\n",
    },
    Row {
        code: SemanticCode::NotAnLvalue,
        broken: "void main() { 1.0 = 2.0; }\n",
        fixed: "void main() { float f; f = 2.0; }\n",
    },
    Row {
        code: SemanticCode::ReadOnly,
        broken: "uniform float f;\nvoid main() { f = 1.0; }\n",
        fixed: "uniform float f;\nvoid main() { float g = f; }\n",
    },
    Row {
        code: SemanticCode::BadOperand,
        broken: "void main() { bool b = true && 1.0; }\n",
        fixed: "void main() { bool b = true && false; }\n",
    },
    Row {
        code: SemanticCode::TypeMismatch,
        broken: "vec2 a; vec3 b;\nvoid main() { vec3 c = a + b; }\n",
        fixed: "vec3 a; vec3 b;\nvoid main() { vec3 c = a + b; }\n",
    },
    Row {
        code: SemanticCode::ConditionNotBool,
        broken: "void main() { if (1.0) { } }\n",
        fixed: "void main() { if (1.0 > 0.0) { } }\n",
    },
    Row {
        code: SemanticCode::ReturnMismatch,
        broken: "float f() { return vec3(1.0); }\n",
        fixed: "float f() { return 1.0; }\n",
    },
    Row {
        code: SemanticCode::DiscardOutsideFragment,
        // The stage has to be *known* for this one, which the fixture below
        // arranges by naming it; with a guessed stage nothing is claimed.
        broken: "void main() { discard; }\n",
        fixed: "void main() { }\n",
    },
    Row {
        code: SemanticCode::MisplacedJump,
        broken: "void main() { break; }\n",
        fixed: "void main() { while (true) { break; } }\n",
    },
    Row {
        code: SemanticCode::ConstInitializer,
        broken: "const float pi;\nvoid main() { }\n",
        fixed: "const float pi = 3.14159;\nvoid main() { }\n",
    },
    Row {
        code: SemanticCode::NotAvailableInVersion,
        broken: "#version 110\nuniform sampler2D s;\n\
                 void main() { gl_FragColor = texture(s, vec2(0.0)); }\n",
        fixed: "#version 110\nuniform sampler2D s;\n\
                void main() { gl_FragColor = texture2D(s, vec2(0.0)); }\n",
    },
    Row {
        code: SemanticCode::NotAvailableInStage,
        // Also stage-dependent; see the fixture below.
        broken: "#version 330\nvoid main() { gl_FragDepth = 1.0; }\n",
        fixed: "#version 330\nvoid main() { }\n",
    },
    Row {
        code: SemanticCode::BadArraySize,
        broken: "void main() { float a[0]; }\n",
        fixed: "void main() { float a[1]; }\n",
    },
    Row {
        code: SemanticCode::UnreachableCode,
        broken: "float f() { return 1.0; float x = 2.0; }\n",
        fixed: "float f() { float x = 2.0; return x; }\n",
    },
    Row {
        code: SemanticCode::MissingReturn,
        broken: "float f() { float x = 1.0; }\n",
        fixed: "float f() { return 1.0; }\n",
    },
];

/// The two rows whose rule only fires when the host has named the stage.
fn analyse_row(row: &Row, source: &str) -> crate::Analysis {
    match row.code {
        SemanticCode::DiscardOutsideFragment => {
            crate::tests::analyze_in(source, glsl_spec::Stage::Vertex)
        }
        SemanticCode::NotAvailableInStage => {
            crate::tests::analyze_in(source, glsl_spec::Stage::Vertex)
        }
        _ => analyze(source),
    }
}

#[test]
fn every_diagnostic_has_a_source_that_produces_exactly_it() {
    for row in CATALOGUE {
        let analysis = analyse_row(row, row.broken);
        assert_eq!(
            codes(&analysis),
            &[row.code.as_str()],
            "{} did not come out of:\n{}\n{:#?}",
            row.code.as_str(),
            row.broken,
            messages(&analysis)
        );
    }
}

#[test]
fn every_fixture_has_a_valid_twin_that_is_silent() {
    for row in CATALOGUE {
        let analysis = analyse_row(row, row.fixed);
        let reported: Vec<String> = analysis
            .diagnostics
            .iter()
            .map(|d| format!("{} {}", d.code.as_str(), d.message))
            .collect();
        assert!(
            reported.is_empty(),
            "the corrected form of {} still reports {reported:#?}\n{}",
            row.code.as_str(),
            row.fixed
        );
    }
}

#[test]
fn the_catalogue_covers_every_code() {
    for code in SemanticCode::ALL {
        assert!(
            CATALOGUE.iter().any(|row| row.code == *code),
            "{} has no fixture; a code nothing can emit is a code nobody can act on",
            code.as_str()
        );
    }
    assert_eq!(CATALOGUE.len(), SemanticCode::ALL.len(), "the catalogue has a duplicate row");
}

#[test]
fn the_codes_are_the_range_phase_4_was_given() {
    let mut seen: Vec<&'static str> = Vec::new();
    for (index, code) in SemanticCode::ALL.iter().enumerate() {
        let text = code.as_str();
        assert_eq!(
            text,
            format!("GLSL{:04}", 200 + index),
            "the codes must be contiguous from GLSL0200 and never renumbered"
        );
        assert!(!seen.contains(&text), "{text} is used twice");
        seen.push(text);
        assert!(!code.summary().is_empty());
    }
}

#[test]
fn only_the_two_warnings_are_warnings() {
    // Severity is part of the contract: everything is an error except the two
    // that describe a file mid-edit rather than a file with a bug.
    for row in CATALOGUE {
        let analysis = analyse_row(row, row.broken);
        let Some(diagnostic) = analysis.diagnostics.first() else {
            continue;
        };
        let expected = match row.code {
            SemanticCode::UnreachableCode | SemanticCode::MissingReturn => Severity::Warning,
            _ => Severity::Error,
        };
        assert_eq!(diagnostic.severity, expected, "{}", row.code.as_str());
    }
}

#[test]
fn every_diagnostic_lands_on_real_source_bytes() {
    for row in CATALOGUE {
        let analysis = analyse_row(row, row.broken);
        for diagnostic in &analysis.diagnostics {
            assert!(
                diagnostic.span.end as usize <= row.broken.len(),
                "{} points past the end of its own fixture",
                row.code.as_str()
            );
            assert!(
                row.broken.is_char_boundary(diagnostic.span.start as usize),
                "{} starts mid-character",
                row.code.as_str()
            );
        }
    }
}

#[test]
fn a_diagnostic_inside_a_macro_lands_on_the_invocation() {
    // Decision 0003's rule, seen from this layer: a body token carries the
    // invocation's span, so an error inside an expansion is painted on text
    // the user can see.
    let source = "#define HALF 0.5\nvoid main() { int i = HALF; }\n";
    let analysis = analyze(source);
    let diagnostic = analysis.errors().next().expect("the narrowing is an error");
    let text = &source[diagnostic.span.start as usize..diagnostic.span.end as usize];
    assert!(text.contains("HALF"), "the error landed on {text:?}");
}
