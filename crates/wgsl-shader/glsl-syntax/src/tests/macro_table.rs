//! P2-03 — the macro table: `#define`, `#undef`, redefinition, predefines.

use pretty_assertions::assert_eq;

use super::{codes, errors, texts};
use crate::diagnostics::PpCode;
use crate::preprocessor::macros::MacroKind;
use crate::preprocess_source;

#[test]
fn an_object_like_define_records_its_body_and_spans() {
    let source = "#define PI 3.14159\nfloat x = PI;\n";
    let pp = preprocess_source(source);
    let def = pp.macros.lookup("PI").expect("PI is defined");
    assert_eq!(def.kind, MacroKind::Object);
    assert!(def.params.is_empty());
    assert_eq!(def.body.iter().map(|t| t.text.as_str()).collect::<Vec<_>>(), ["3.14159"]);
    // The spans go-to-definition will use: the whole directive, and the name.
    assert_eq!(&source[def.span.start as usize..def.span.end as usize], "#define PI 3.14159");
    assert_eq!(&source[def.name_span.start as usize..def.name_span.end as usize], "PI");
}

#[test]
fn a_define_with_an_empty_body_expands_to_nothing() {
    let pp = preprocess_source("#define NOTHING\nint a = NOTHING 1;\n");
    assert_eq!(errors(&pp), Vec::<&str>::new());
    assert_eq!(texts(&pp), ["int", "a", "=", "1", ";"]);
}

#[test]
fn a_function_like_define_records_its_parameters() {
    let pp = preprocess_source("#define ADD(a, b) ((a) + (b))\n");
    let def = pp.macros.lookup("ADD").expect("ADD is defined");
    assert_eq!(def.kind, MacroKind::Function);
    assert_eq!(def.params, ["a", "b"]);
}

#[test]
fn a_space_before_the_paren_makes_the_macro_object_like() {
    // `#define F (x) …` defines `F` as `(x) …`; only `#define F(x)` takes
    // parameters. The space is the whole difference.
    let object = preprocess_source("#define F (x)\nint a = F;\n");
    assert_eq!(object.macros.lookup("F").unwrap().kind, MacroKind::Object);
    assert_eq!(texts(&object), ["int", "a", "=", "(", "x", ")", ";"]);
    let function = preprocess_source("#define F(x) x\nint a = F(9);\n");
    assert_eq!(function.macros.lookup("F").unwrap().kind, MacroKind::Function);
    assert_eq!(texts(&function), ["int", "a", "=", "9", ";"]);
}

#[test]
fn a_function_like_macro_may_take_no_parameters() {
    let pp = preprocess_source("#define NOW() 7\nint a = NOW();\n");
    let def = pp.macros.lookup("NOW").expect("NOW is defined");
    assert_eq!(def.kind, MacroKind::Function);
    assert!(def.params.is_empty());
    assert_eq!(texts(&pp), ["int", "a", "=", "7", ";"]);
}

#[test]
fn undef_retires_a_macro_but_keeps_it_findable() {
    let source = "#define X 1\n#undef X\nint a = X;\n";
    let pp = preprocess_source(source);
    assert_eq!(errors(&pp), Vec::<&str>::new());
    assert!(pp.macros.lookup("X").is_none());
    // Still in the history, with the span that retired it, so go-to-definition
    // answers for the lines that used it before the `#undef`.
    let retired = pp.macros.all().iter().find(|d| d.name == "X").expect("X was defined once");
    assert!(retired.undefined_at.is_some());
    assert_eq!(texts(&pp), ["int", "a", "=", "X", ";"]);
}

#[test]
fn undef_of_something_never_defined_is_not_an_error() {
    let pp = preprocess_source("#undef NEVER\n");
    assert_eq!(errors(&pp), Vec::<&str>::new());
}

#[test]
fn redefining_a_macro_identically_is_allowed() {
    let pp = preprocess_source("#define X 1 + 2\n#define X 1 + 2\n");
    assert_eq!(errors(&pp), Vec::<&str>::new());
}

#[test]
fn redefining_a_macro_differently_is_an_error() {
    for source in [
        "#define X 1\n#define X 2\n",
        "#define X 1\n#define X 1 2\n",
        "#define F(a) a\n#define F(b) b\n",
        "#define F(a) a\n#define F(a, b) a\n",
        "#define F(a) a\n#define F a\n",
        // Whitespace separation counts: `1 + 2` and `1 +2` differ.
        "#define X 1 + 2\n#define X 1 +2\n",
    ] {
        assert_eq!(codes(&preprocess_source(source)), [PpCode::MacroRedefined], "for {source:?}");
    }
}

#[test]
fn leading_whitespace_on_the_first_body_token_does_not_count() {
    let pp = preprocess_source("#define X 1\n#define X    1\n");
    assert_eq!(errors(&pp), Vec::<&str>::new());
}

#[test]
fn redefinition_after_undef_is_allowed() {
    let pp = preprocess_source("#define X 1\n#undef X\n#define X 2\nint a = X;\n");
    assert_eq!(errors(&pp), Vec::<&str>::new());
    assert_eq!(texts(&pp), ["int", "a", "=", "2", ";"]);
    // Both definitions are in the history, in source order.
    let all: Vec<_> = pp.macros.all().iter().filter(|d| d.name == "X").collect();
    assert_eq!(all.len(), 2);
}

#[test]
fn a_macro_defined_between_two_uses_only_affects_the_second() {
    let pp = preprocess_source("int a = X;\n#define X 1\nint b = X;\n");
    assert_eq!(texts(&pp), ["int", "a", "=", "X", ";", "int", "b", "=", "1", ";"]);
}

#[test]
fn gl_prefixed_names_may_not_be_defined() {
    let pp = preprocess_source("#define GL_MY_EXT 1\n");
    assert_eq!(codes(&pp), [PpCode::ReservedMacroName]);
    assert!(pp.macros.lookup("GL_MY_EXT").is_none());
    assert_eq!(codes(&preprocess_source("#undef GL_ES\n")), [PpCode::ReservedMacroName]);
}

#[test]
fn defined_may_not_be_defined() {
    assert_eq!(codes(&preprocess_source("#define defined 1\n")), [PpCode::ReservedMacroName]);
}

#[test]
fn double_underscore_names_are_only_a_warning() {
    // §3.3 reserves them but says defining one is undefined behaviour rather
    // than an error, and glslang agrees.
    let pp = preprocess_source("#define MY__NAME 1\nint a = MY__NAME;\n");
    assert_eq!(codes(&pp), [PpCode::ReservedMacroNameWarning]);
    assert_eq!(errors(&pp), Vec::<&str>::new());
    assert_eq!(texts(&pp), ["int", "a", "=", "1", ";"]);
}

#[test]
fn the_dynamic_predefines_may_not_be_redefined() {
    for name in ["__LINE__", "__FILE__", "__VERSION__"] {
        let pp = preprocess_source(&format!("#define {name} 1\n"));
        assert_eq!(codes(&pp), [PpCode::ReservedMacroName], "for {name}");
    }
}

#[test]
fn line_file_and_version_have_values() {
    let source = "#version 450 core\nint a = __LINE__;\nint b = __FILE__;\nint c = __VERSION__;\n";
    let pp = preprocess_source(source);
    assert_eq!(texts(&pp), [
        "int", "a", "=", "2", ";", //
        "int", "b", "=", "0", ";", //
        "int", "c", "=", "450", ";",
    ]);
}

#[test]
fn line_counts_from_one() {
    let pp = preprocess_source("int a = __LINE__;\n");
    assert_eq!(texts(&pp), ["int", "a", "=", "1", ";"]);
}

#[test]
fn the_predefines_are_in_the_table_and_marked_as_such() {
    let pp = preprocess_source("#version 100\n");
    for name in ["__LINE__", "__FILE__", "__VERSION__"] {
        let def = pp.macros.lookup(name).unwrap_or_else(|| panic!("{name} is predefined"));
        assert_eq!(def.kind, MacroKind::Dynamic);
        assert!(def.predefined);
    }
    let gl_es = pp.macros.lookup("GL_ES").expect("GL_ES is defined for ES");
    assert!(gl_es.predefined);
    assert_eq!(gl_es.kind, MacroKind::Object);
}

#[test]
fn a_missing_macro_name_is_diagnosed_rather_than_panicking() {
    assert_eq!(codes(&preprocess_source("#define\n")), [PpCode::MissingMacroName]);
    assert_eq!(codes(&preprocess_source("#undef\n")), [PpCode::MissingMacroName]);
    assert_eq!(codes(&preprocess_source("#define 1 2\n")), [PpCode::MissingMacroName]);
}

#[test]
fn a_broken_parameter_list_is_diagnosed_rather_than_panicking() {
    assert_eq!(codes(&preprocess_source("#define F(a\n")), [PpCode::UnterminatedMacroParameters]);
    assert_eq!(codes(&preprocess_source("#define F(a,)\n")), [PpCode::BadMacroParameter]);
    assert_eq!(codes(&preprocess_source("#define F(1) x\n")), [PpCode::BadMacroParameter]);
    assert_eq!(
        codes(&preprocess_source("#define F(a, a) a\n")),
        [PpCode::DuplicateMacroParameter]
    );
}

#[test]
fn host_predefines_behave_like_source_ones() {
    use crate::preprocessor::{PreprocessOptions, preprocess};

    let source = "#ifdef HOST\nint a = HOST;\n#endif\n";
    let options = PreprocessOptions {
        predefines: vec![("HOST".to_string(), "42".to_string())],
    };
    let pp = preprocess(source, &crate::tokenize(source), &options);
    assert_eq!(texts(&pp), ["int", "a", "=", "42", ";"]);
    assert!(pp.macros.lookup("HOST").expect("HOST is defined").predefined);
}
