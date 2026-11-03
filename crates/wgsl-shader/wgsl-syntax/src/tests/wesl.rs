//! WESL: the WGSL walk with imports, module paths and conditional translation.

use super::{at, find, names};
use crate::lexer::TokenKind;
use crate::tree::{PathRoot, SymbolKind};
use crate::{Language, Parsed, parse};

fn wesl(source: &str) -> Parsed {
    parse(source, Language::Wesl)
}

/// The imports' paths as written, with any rename.
fn paths(parsed: &Parsed) -> Vec<String> {
    parsed
        .imports
        .iter()
        .map(|import| match &import.alias {
            Some((alias, _)) => format!("{} as {alias}", import.path()),
            None => import.path(),
        })
        .collect()
}

#[test]
fn an_import_brings_its_last_segment_into_scope() {
    let parsed = wesl("import package::lighting::attenuate;\nfn f() { attenuate(); }\n");
    assert_eq!(paths(&parsed), ["package::lighting::attenuate"]);
    assert_eq!(names(&parsed, SymbolKind::Import), ["attenuate"]);

    let import = &parsed.imports[0];
    assert_eq!(import.root, PathRoot::Package);
    assert_eq!(parsed.symbols[import.symbol].detail, "import package::lighting::attenuate");
    // The use in the body resolves to the import.
    let use_site = parsed.references.last().unwrap();
    assert_eq!(parsed.resolve_name("attenuate", use_site.span.start), Some(import.symbol));
}

#[test]
fn collections_nest_and_renames_take_the_alias() {
    let source = "import package::lights::{point, spot as cone, shadow::{pcf, vsm,}};\n";
    let parsed = wesl(source);
    assert_eq!(
        paths(&parsed),
        [
            "package::lights::point",
            "package::lights::spot as cone",
            "package::lights::shadow::pcf",
            "package::lights::shadow::vsm",
        ]
    );
    assert_eq!(names(&parsed, SymbolKind::Import), ["point", "cone", "pcf", "vsm"]);
    // The alias, not the path segment, is the declaration.
    let cone = find(&parsed, "cone");
    assert_eq!(&source[cone.name_span.start as usize..cone.name_span.end as usize], "cone");
}

#[test]
fn super_counts_its_repeats() {
    let parsed = wesl("import super::super::util::hash;\nimport random::pcg;\n");
    assert_eq!(parsed.imports[0].root, PathRoot::Super(2));
    assert_eq!(parsed.imports[0].path(), "super::super::util::hash");
    // A bare first segment names a dependency package.
    assert_eq!(parsed.imports[1].root, PathRoot::Bare);
    assert_eq!(parsed.imports[1].path(), "random::pcg");
}

#[test]
fn the_import_words_are_keywords_only_in_wesl() {
    let classify = |language| {
        crate::lexer::tokenize("import package super as", language)
            .iter()
            .map(|t| t.kind)
            .collect::<Vec<_>>()
    };
    assert!(classify(Language::Wesl).iter().all(|&k| k == TokenKind::Keyword));
    assert!(classify(Language::Wgsl).iter().all(|&k| k == TokenKind::Ident));
}

/// A WGSL file has no imports, so the word is just a name.
#[test]
fn wgsl_does_not_parse_imports() {
    let parsed = parse("import package::a::b;\nfn f() {}\n", Language::Wgsl);
    assert!(parsed.imports.is_empty());
    assert_eq!(names(&parsed, SymbolKind::Function), ["f"]);
}

/// The statement being typed has no `;` yet, and must not take the function
/// after it down with it.
#[test]
fn an_unterminated_import_ends_at_the_next_declaration() {
    let parsed = wesl("import package::lights::point\nfn shade() {}\n");
    assert_eq!(paths(&parsed), ["package::lights::point"]);
    assert_eq!(names(&parsed, SymbolKind::Function), ["shade"]);
}

#[test]
fn a_name_after_a_path_separator_is_a_segment_not_a_local_name() {
    let (parsed, text, offset) =
        at("import package::utils;\nfn f() { let x = utils::he|lper(); }\n", Language::Wesl);
    let reference = parsed.reference_at(offset).unwrap();
    assert!(reference.is_member);
    assert_eq!(parsed.resolve_at(&text, offset), None);
    // …while the module name in front of it is an ordinary use of the import.
    let utils = text.rfind("utils").unwrap() as u32;
    assert!(!parsed.reference_at(utils).unwrap().is_member);
    let resolved = parsed.resolve_at(&text, utils).map(|i| parsed.symbols[i].kind);
    assert_eq!(resolved, Some(SymbolKind::Import));
}

#[test]
fn the_path_prefix_reads_back_through_a_qualified_name() {
    let (parsed, text, offset) = at("fn f() { package::lights::|point(); }\n", Language::Wesl);
    let prefix = parsed.path_prefix(&text, offset).unwrap();
    assert_eq!(prefix.root, PathRoot::Package);
    let segments: Vec<&str> = prefix.segments.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(segments, ["lights"]);

    let (parsed, text, offset) = at("fn f() { utils::|", Language::Wesl);
    let prefix = parsed.path_prefix(&text, offset).unwrap();
    assert_eq!(prefix.root, PathRoot::Bare);
    assert_eq!(prefix.segments[0].0, "utils");

    let (parsed, text, offset) = at("fn f() { let a = |b; }\n", Language::Wesl);
    assert_eq!(parsed.path_prefix(&text, offset), None);
}

/// Completion inside `{…}` needs the path the collection hangs off.
#[test]
fn the_path_prefix_inside_a_collection_includes_what_it_hangs_off() {
    let (parsed, text, offset) =
        at("import super::lights::{point, shadow::{pcf, |}};\n", Language::Wesl);
    let prefix = parsed.path_prefix(&text, offset).unwrap();
    assert_eq!(prefix.root, PathRoot::Super(1));
    let segments: Vec<&str> = prefix.segments.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(segments, ["lights", "shadow"]);
}

/// A call's argument list is not an import collection, however much a `,`
/// looks like one.
#[test]
fn a_comma_in_an_argument_list_starts_no_path() {
    let (parsed, text, offset) = at("fn f() { g(a, |b); }\n", Language::Wesl);
    assert_eq!(parsed.path_prefix(&text, offset), None);
}

#[test]
fn conditional_declarations_may_share_a_name() {
    let parsed = wesl(
        "@if(mobile) const SAMPLES = 4;\n@else const SAMPLES = 16;\n\
         struct Light {\n    @if(hdr) colour: vec4f,\n    @else colour: vec3f,\n}\n\
         fn f() {\n    @if(fast) let n = 1;\n    @else let n = 2;\n}\n",
    );
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    assert_eq!(names(&parsed, SymbolKind::Constant), ["SAMPLES", "SAMPLES"]);
}

/// …but an unconditional repeat is still the error it always was.
#[test]
fn an_unconditional_repeat_is_still_reported() {
    let parsed = wesl("@if(mobile) const A = 1;\nconst B = 1;\nconst B = 2;\n");
    let messages: Vec<&str> = parsed.diagnostics.iter().map(|d| d.message.as_str()).collect();
    assert_eq!(messages, ["`B` is already declared in this scope"]);
}

#[test]
fn a_conditional_group_of_declarations_is_outlined() {
    let parsed = wesl(
        "@if(shadows) {\n    const CASCADES = 4;\n    fn sample_shadow() {}\n}\n\
         @else {\n    const CASCADES = 0;\n}\nfn main() {}\n",
    );
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    assert_eq!(names(&parsed, SymbolKind::Constant), ["CASCADES", "CASCADES"]);
    assert_eq!(names(&parsed, SymbolKind::Function), ["sample_shadow", "main"]);
}

#[test]
fn a_conditional_import_is_still_an_import() {
    let parsed = wesl("@if(debug) import package::debug::trace;\nfn f() {}\n");
    assert_eq!(paths(&parsed), ["package::debug::trace"]);
    assert_eq!(names(&parsed, SymbolKind::Function), ["f"]);
}
