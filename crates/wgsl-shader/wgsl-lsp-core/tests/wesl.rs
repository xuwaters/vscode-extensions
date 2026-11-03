//! WESL end to end: imports followed across files, through the same three
//! methods the WASM binding wraps.
//!
//! Fixture files live under `file:///shaders/`, so with no `wesl.toml` a
//! file's own directory is its package root — `package::lighting` in
//! `main.wesl` is `lighting.wesl` beside it.

mod support;

use lsp_types::{
    CompletionItemKind, CompletionResponse, Diagnostic, DocumentSymbolResponse,
    GotoDefinitionResponse, Hover, HoverContents, Location, MarkupContent, Position,
    SemanticTokensResult, Uri,
};
use serde_json::json;
use support::{Harness, find, uri_for};

const LIGHTING: &str = "\
/// Inverse-square falloff.
fn attenuate(distance: f32) -> f32 {
    return 1.0 / (distance * distance);
}

struct Light {
    colour: vec3f,
}
";

const MAIN: &str = "\
import package::lighting::{attenuate, Light};
import package::lighting;
import package::lighting::attenuate as falloff;

fn shade(light: Light, d: f32) -> vec3f {
    return light.colour * attenuate(d) * lighting::attenuate(d) * falloff(d);
}
";

/// A workspace holding `lighting.wesl`, with `main.wesl` open.
fn workspace() -> (Harness, Uri) {
    let mut harness = Harness::new();
    harness.workspace_files(&[("lighting.wesl", LIGHTING)]);
    let uri = harness.open("main.wesl", MAIN);
    (harness, uri)
}

fn definition(harness: &mut Harness, uri: &Uri, position: Position) -> Option<Location> {
    match harness.at::<GotoDefinitionResponse>("textDocument/definition", uri, position)? {
        GotoDefinitionResponse::Scalar(location) => Some(location),
        GotoDefinitionResponse::Array(mut locations) => locations.pop(),
        GotoDefinitionResponse::Link(_) => panic!("links are not used"),
    }
}

fn hover(harness: &mut Harness, uri: &Uri, position: Position) -> String {
    let hover: Hover = harness.at("textDocument/hover", uri, position).expect("a hover");
    match hover.contents {
        HoverContents::Markup(MarkupContent { value, .. }) => value,
        other => panic!("unexpected hover contents: {other:?}"),
    }
}

fn completion(harness: &mut Harness, name: &str, text: &str) -> Vec<(String, CompletionItemKind)> {
    let (uri, position) = harness.open_at(name, text);
    let response: CompletionResponse =
        harness.at("textDocument/completion", &uri, position).expect("a completion list");
    let CompletionResponse::Array(items) = response else { panic!("expected an array") };
    items.into_iter().map(|item| (item.label, item.kind.unwrap())).collect()
}

fn messages(diagnostics: &[Diagnostic]) -> Vec<&str> {
    diagnostics.iter().map(|d| d.message.as_str()).collect()
}

/// The text a range covers in a one-file fixture.
fn text_at<'a>(text: &'a str, location: &Location) -> &'a str {
    let line = text.lines().nth(location.range.start.line as usize).unwrap();
    &line[location.range.start.character as usize..location.range.end.character as usize]
}

// ── Definition ─────────────────────────────────────────────────────────────

#[test]
fn an_imported_name_leads_to_its_declaration_in_the_other_file() {
    let (mut harness, uri) = workspace();
    // The use in the body, not the import line.
    let use_site = find(MAIN, "attenuate(d) *", 0);
    let location = definition(&mut harness, &uri, use_site).expect("a definition");
    assert_eq!(location.uri, uri_for("lighting.wesl"));
    assert_eq!(text_at(LIGHTING, &location), "attenuate");
}

#[test]
fn a_qualified_name_and_an_alias_lead_there_too() {
    let (mut harness, uri) = workspace();
    let qualified = find(MAIN, "lighting::attenuate(d)", "lighting::".len());
    let alias = find(MAIN, "falloff(d)", 2);
    for (needle, position) in [("qualified", qualified), ("alias", alias)] {
        let location = definition(&mut harness, &uri, position).expect(needle);
        assert_eq!(location.uri, uri_for("lighting.wesl"), "{needle}");
        assert_eq!(text_at(LIGHTING, &location), "attenuate", "{needle}");
    }
}

#[test]
fn a_module_segment_leads_to_the_module_file() {
    let (mut harness, uri) = workspace();
    let location = definition(&mut harness, &uri, find(MAIN, "lighting::{", 1)).unwrap();
    assert_eq!(location.uri, uri_for("lighting.wesl"));
    assert_eq!(location.range.start, Position::new(0, 0));
}

/// An open document answers for itself, unsaved edits and all.
#[test]
fn an_open_module_is_read_as_it_stands_in_the_editor() {
    let mut harness = Harness::new();
    harness.workspace_files(&[("lighting.wesl", LIGHTING)]);
    let lighting = harness.open("lighting.wesl", LIGHTING);
    harness.change(&lighting, &format!("{LIGHTING}fn fresh() {{}}\n"));

    let source = "import package::lighting::fresh;\nfn f() { fresh(); }\n";
    let uri = harness.open("main.wesl", source);
    let location = definition(&mut harness, &uri, find(source, "fresh();", 0)).unwrap();
    assert_eq!(location.uri, lighting);
    assert!(harness.diagnostics(&uri).is_none_or(|d| d.is_empty()));
}

// ── Packages ───────────────────────────────────────────────────────────────

/// `wesl.toml` moves `package::` to its `root`, `super::` climbs from the
/// file's own module, and a path dependency is a folder of its own.
#[test]
fn a_manifest_sets_the_root_and_its_path_dependencies() {
    let mut harness = Harness::new();
    harness.workspace_files(&[
        (
            "app/wesl.toml",
            "edition = \"2026_pre\"\nroot = \"src\"\n\
             [dependencies]\nnoise = { path = \"../noise\" }\n",
        ),
        ("app/src/util.wesl", "fn helper() {}\n"),
        ("app/src/lights/shadow.wesl", "fn pcf() {}\n"),
        ("noise/wesl.toml", "edition = \"2026_pre\"\nroot = \"lib\"\n"),
        ("noise/lib/perlin.wesl", "fn perlin() -> f32 { return 0.0; }\n"),
    ]);
    let source = "\
import package::util::helper;
import super::shadow::pcf;
import noise::perlin::perlin;
fn f() { helper(); pcf(); perlin(); }
";
    let uri = harness.open("app/src/lights/point.wesl", source);
    assert_eq!(harness.diagnostics(&uri), None, "every import resolves");

    for (needle, file) in [
        ("helper();", "app/src/util.wesl"),
        ("pcf();", "app/src/lights/shadow.wesl"),
        ("perlin();", "noise/lib/perlin.wesl"),
    ] {
        let location = definition(&mut harness, &uri, find(source, needle, 0)).expect(needle);
        assert_eq!(location.uri, uri_for(file), "{needle}");
    }
}

// ── Diagnostics ────────────────────────────────────────────────────────────

#[test]
fn an_import_that_names_nothing_is_reported_on_the_segment_that_fails() {
    let mut harness = Harness::new();
    // A package rooted at the fixtures' own directory, so `super` has a
    // floor to hit.
    harness.workspace_files(&[("wesl.toml", "root = \".\"\n"), ("lighting.wesl", LIGHTING)]);
    let source = "\
import package::lighting::missing;
import package::nowhere::thing;
import super::super::too_far;
import some_dependency::anything;
fn f() { lighting::nope(); }
import package::lighting;
";
    let uri = harness.open("main.wesl", source);
    let diagnostics = harness.diagnostics(&uri).expect("diagnostics");
    assert_eq!(
        messages(&diagnostics),
        [
            "cannot find `missing` in `package::lighting`",
            "cannot find `nowhere` in `package`",
            "too many `super`s: this climbs above the package root",
            "cannot find `nope` in `package::lighting`",
        ],
    );
    // On the segment, not the statement.
    assert_eq!(diagnostics[0].range.start, find(source, "missing", 0));
    assert_eq!(diagnostics[3].range.start, find(source, "nope", 0));
    assert!(diagnostics.iter().all(|d| d.source.as_deref() == Some("wesl")));
}

/// `super` is held to the package's depth only inside a package: with no
/// `wesl.toml` above it, a file's root is its own directory and climbing out
/// is what a loose collection of files does.
#[test]
fn super_climbs_freely_outside_a_package() {
    let mut harness = Harness::new();
    harness.workspace_files(&[("shared.wesl", "fn helper() {}\n")]);
    // `super` is `lights`, the module the file sits in; its parent is the
    // directory above, which no package root contains.
    let uri = harness.open(
        "lights/point.wesl",
        "import super::super::shared::helper;\nfn f() { helper(); }\n",
    );
    assert_eq!(harness.diagnostics(&uri), None);
}

/// Saving the module an open file imports from settles its diagnostics.
#[test]
fn saving_the_imported_module_republishes_its_importers() {
    let mut harness = Harness::new();
    let lighting = harness.open("lighting.wesl", LIGHTING);
    let source = "import package::lighting::fresh;\nfn f() { fresh(); }\n";
    let uri = harness.open("main.wesl", source);
    assert_eq!(
        messages(&harness.diagnostics(&uri).unwrap()),
        ["cannot find `fresh` in `package::lighting`"]
    );

    harness.change(&lighting, &format!("{LIGHTING}fn fresh() {{}}\n"));
    harness.save(&lighting);
    assert_eq!(harness.diagnostics(&uri), Some(Vec::new()));
}

/// A WESL file that uses nothing WGSL lacks is WGSL, and naga checks it in
/// full; one that imports is not one naga can read, so it is not asked.
#[test]
fn naga_validates_a_wesl_file_only_when_it_is_plain_wgsl() {
    let mut harness = Harness::new();
    let plain = harness.open("plain.wesl", "fn f() -> f32 { return true; }\n");
    let diagnostics = harness.diagnostics(&plain).expect("naga found the mismatch");
    assert!(diagnostics.iter().any(|d| d.source.as_deref() == Some("naga")), "{diagnostics:?}");

    harness.workspace_files(&[("lighting.wesl", LIGHTING)]);
    let importer = harness.open(
        "importer.wesl",
        "import package::lighting::attenuate;\nfn f() -> f32 { return attenuate(2.0); }\n",
    );
    assert_eq!(harness.diagnostics(&importer), None);
}

#[test]
fn conditional_declarations_are_not_duplicates() {
    let mut harness = Harness::new();
    let uri = harness.open(
        "a.wesl",
        "@if(mobile) const SAMPLES = 4;\n@else const SAMPLES = 16;\n\
         fn f() -> i32 { return SAMPLES; }\n",
    );
    assert_eq!(harness.diagnostics(&uri), None);
}

// ── Hover ──────────────────────────────────────────────────────────────────

#[test]
fn hovering_an_imported_name_shows_its_declaration_and_doc() {
    let (mut harness, uri) = workspace();
    let markdown = hover(&mut harness, &uri, find(MAIN, "falloff(d)", 0));
    assert_eq!(
        markdown,
        "```wesl\nfn attenuate(distance: f32) -> f32\n```\n\nFrom `package::lighting`.\n\n\
         Inverse-square falloff."
    );
}

#[test]
fn hovering_a_module_names_its_file() {
    let (mut harness, uri) = workspace();
    let markdown = hover(&mut harness, &uri, find(MAIN, "lighting;", 0));
    assert_eq!(markdown, "```wesl\nmodule package::lighting\n```\n\nA module, in `lighting.wesl`.");
}

// ── Completion ─────────────────────────────────────────────────────────────

#[test]
fn completing_a_path_offers_what_the_module_holds() {
    let mut harness = Harness::new();
    harness.workspace_files(&[("lighting.wesl", LIGHTING), ("lights/point.wesl", "fn p() {}\n")]);

    let modules = completion(&mut harness, "main.wesl", "import package::|");
    assert!(modules.contains(&("lighting".to_string(), CompletionItemKind::MODULE)), "{modules:?}");
    assert!(modules.contains(&("lights".to_string(), CompletionItemKind::MODULE)), "{modules:?}");
    // The file being edited is a module too, but importing yourself is not
    // a thing anyone means — it is offered, harmlessly, like any sibling.
    let items = completion(&mut harness, "other.wesl", "import package::lighting::{attenuate, |}");
    assert_eq!(
        items,
        [
            ("attenuate".to_string(), CompletionItemKind::FUNCTION),
            ("Light".to_string(), CompletionItemKind::STRUCT),
        ]
    );
}

#[test]
fn completing_after_an_imported_module_name_offers_its_declarations() {
    let mut harness = Harness::new();
    harness.workspace_files(&[("lighting.wesl", LIGHTING)]);
    let source = "import package::lighting;\nfn f() { lighting::| }\n";
    let items = completion(&mut harness, "main.wesl", source);
    let labels: Vec<&str> = items.iter().map(|(label, _)| label.as_str()).collect();
    assert_eq!(labels, ["attenuate", "Light"]);
}

#[test]
fn completing_after_import_offers_where_a_path_can_start() {
    let mut harness = Harness::new();
    let items = completion(&mut harness, "main.wesl", "import |");
    let labels: Vec<&str> = items.iter().map(|(label, _)| label.as_str()).collect();
    assert_eq!(labels, ["package", "super"]);
}

/// In the general list an imported function is a function, not an import.
#[test]
fn an_imported_name_completes_as_what_it_names() {
    let mut harness = Harness::new();
    harness.workspace_files(&[("lighting.wesl", LIGHTING)]);
    let source = "import package::lighting::attenuate;\nfn f() { att| }\n";
    let items = completion(&mut harness, "main.wesl", source);
    assert!(items.contains(&("attenuate".to_string(), CompletionItemKind::FUNCTION)), "{items:?}");
}

#[test]
fn the_wesl_attributes_complete_after_at() {
    let mut harness = Harness::new();
    let items = completion(&mut harness, "main.wesl", "@|");
    let labels: Vec<&str> = items.iter().map(|(label, _)| label.as_str()).collect();
    assert!(labels.contains(&"if") && labels.contains(&"elif") && labels.contains(&"else"));

    // WGSL has no conditional translation.
    let items = completion(&mut harness, "main.wgsl", "@|");
    assert!(!items.iter().any(|(label, _)| label == "elif"));
}

/// `:` triggers completion for `::`. A lone `:` is a type annotation, and
/// popping a list there would be new and unasked-for in WGSL.
#[test]
fn a_lone_colon_trigger_offers_nothing() {
    let mut harness = Harness::new();
    let (uri, position) = harness.open_at("a.wgsl", "struct S { a:| }\n");
    let response: Option<CompletionResponse> = harness.request(
        "textDocument/completion",
        json!({
            "textDocument": { "uri": uri.as_str() },
            "position": position,
            "context": { "triggerKind": 2, "triggerCharacter": ":" },
        }),
    );
    assert!(response.is_none());
}

// ── Editing ────────────────────────────────────────────────────────────────

/// The name is also the import's last segment: renaming it here would break
/// the path. An `as` alias is this file's to rename.
#[test]
fn an_imported_name_is_renamed_only_through_an_alias() {
    let (mut harness, uri) = workspace();
    let refused: Option<serde_json::Value> =
        harness.at("textDocument/prepareRename", &uri, find(MAIN, "attenuate(d) *", 0));
    assert!(refused.is_none());
    let allowed: Option<serde_json::Value> =
        harness.at("textDocument/prepareRename", &uri, find(MAIN, "falloff(d)", 0));
    assert!(allowed.is_some());
}

#[test]
fn imports_are_left_out_of_the_outline() {
    let (mut harness, uri) = workspace();
    let response: DocumentSymbolResponse = harness
        .request("textDocument/documentSymbol", json!({ "textDocument": { "uri": uri.as_str() } }))
        .unwrap();
    let DocumentSymbolResponse::Nested(symbols) = response else { panic!("expected nested") };
    let names: Vec<&str> = symbols.iter().map(|symbol| symbol.name.as_str()).collect();
    assert_eq!(names, ["shade"]);
}

/// An imported function paints as a function and a module as a namespace —
/// what each *is* in the file it comes from.
#[test]
fn semantic_tokens_paint_imports_as_what_they_name() {
    let (mut harness, uri) = workspace();
    let result: SemanticTokensResult = harness
        .request(
            "textDocument/semanticTokens/full",
            json!({ "textDocument": { "uri": uri.as_str() } }),
        )
        .unwrap();
    let SemanticTokensResult::Tokens(tokens) = result else { panic!("expected a full array") };

    // Rebuild absolute positions so a token can be found by where it starts.
    let (mut line, mut start) = (0, 0);
    let mut at = std::collections::HashMap::new();
    for token in &tokens.data {
        if token.delta_line != 0 {
            start = 0;
        }
        line += token.delta_line;
        start += token.delta_start;
        at.insert((line, start), token.token_type);
    }
    let type_at = |position: Position| at[&(position.line, position.character)];

    // See `semantic_tokens::TYPES`: 2 STRUCT, 3 FUNCTION, 13 NAMESPACE.
    assert_eq!(type_at(find(MAIN, "attenuate(d) *", 0)), 3);
    assert_eq!(type_at(find(MAIN, "Light, d", 0)), 2);
    assert_eq!(type_at(find(MAIN, "lighting::attenuate(d)", 0)), 13);
    assert_eq!(type_at(find(MAIN, "falloff(d)", 0)), 3);
}

/// The example package the extension ships resolves without a complaint.
#[test]
fn the_shipped_example_package_resolves() {
    const EXAMPLE: &str = "../../../extensions/wgsl-shader/examples/wesl";
    let read = |name: &str| {
        std::fs::read_to_string(format!("{}/{EXAMPLE}/{name}", env!("CARGO_MANIFEST_DIR")))
            .unwrap_or_else(|error| panic!("{name}: {error}"))
    };
    let mut harness = Harness::new();
    let manifest = read("wesl.toml");
    let lighting = read("shaders/lighting.wesl");
    let point = read("shaders/lights/point.wesl");
    harness.workspace_files(&[
        ("example/wesl.toml", &manifest),
        ("example/shaders/lighting.wesl", &lighting),
        ("example/shaders/lights/point.wesl", &point),
    ]);
    let main = read("shaders/main.wesl");
    let uri = harness.open("example/shaders/main.wesl", &main);
    assert_eq!(harness.diagnostics(&uri), None);
    let point_uri = harness.open("example/shaders/lights/point.wesl", &point);
    assert_eq!(harness.diagnostics(&point_uri), None);

    let location = definition(&mut harness, &uri, find(&main, "shade(", 0)).unwrap();
    assert_eq!(location.uri, point_uri);
}
