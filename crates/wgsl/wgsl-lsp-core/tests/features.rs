//! End-to-end feature tests.
//!
//! Everything goes through [`support::Harness`], which wraps the same three
//! methods the WASM binding does — so these exercise dispatch, parameter
//! deserialization and the handler together, in the shape a real client uses.

mod support;

use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionResponse, DocumentSymbol,
    DocumentSymbolResponse, FoldingRange, GotoDefinitionResponse, Hover, HoverContents,
    InlayHint, InlayHintLabel, Location, MarkupContent, Position, PrepareRenameResponse, Range,
    SemanticTokensFullDeltaResult, SemanticTokensResult, SignatureHelp, TextEdit,
    WorkspaceEdit, WorkspaceSymbolResponse,
};
use serde_json::json;
use support::{Harness, find, uri_for};
use wgsl_lsp_core::Settings;

const WGSL: &str = r#"
/// The camera, as the pipeline binds it.
struct Camera {
    view: mat4x4f,
    eye: vec3f,
}

@group(0) @binding(0) var<uniform> camera: Camera;

const AMBIENT: f32 = 0.1;

/// Inverse-square falloff.
fn attenuate(distance: f32, falloff: f32) -> f32 {
    let squared = distance * distance;
    return 1.0 / (squared * falloff);
}

@fragment
fn fs_main(@location(0) uv: vec2f) -> @location(0) vec4f {
    var total = AMBIENT;
    total = total + attenuate(camera.eye.x, 2.0);
    return vec4f(total, uv.x, uv.y, 1.0);
}
"#;

const GLSL: &str = r#"#version 450
#define MAX_LIGHTS 4

layout(std140, binding = 0) uniform Camera {
    mat4 view;
    vec3 eye;
} camera;

layout(location = 0) in vec3 position;
layout(location = 0) out vec4 fragColour;

struct Light {
    vec3 colour;
    float intensity;
};

layout(binding = 1) uniform Light light;

const float AMBIENT = 0.1;

// Inverse-square falloff.
float attenuate(float distance, float falloff) {
    float squared = distance * distance;
    return 1.0 / (squared * falloff);
}

void main() {
    float total = AMBIENT;
    total += attenuate(camera.eye.x, 2.0);
    fragColour = vec4(total * position * light.colour, 1.0);
}
"#;

fn hover_text(hover: Hover) -> String {
    match hover.contents {
        HoverContents::Markup(MarkupContent { value, .. }) => value,
        other => panic!("expected markup, got {other:?}"),
    }
}

fn items(response: CompletionResponse) -> Vec<CompletionItem> {
    match response {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    }
}

fn labels(response: CompletionResponse) -> Vec<String> {
    items(response).into_iter().map(|item| item.label).collect()
}

fn locations(response: GotoDefinitionResponse) -> Vec<Location> {
    match response {
        GotoDefinitionResponse::Scalar(location) => vec![location],
        GotoDefinitionResponse::Array(locations) => locations,
        GotoDefinitionResponse::Link(links) => links
            .into_iter()
            .map(|link| Location { uri: link.target_uri, range: link.target_selection_range })
            .collect(),
    }
}

// ── Diagnostics ────────────────────────────────────────────────────────────

#[test]
fn a_clean_shader_publishes_nothing_and_a_broken_one_publishes_an_error() {
    let mut harness = Harness::new();
    let clean = harness.open("clean.wgsl", WGSL);
    assert!(harness.diagnostics(&clean).is_none());

    let broken = harness.open("broken.wgsl", "fn main() {\n    let x = nope();\n}\n");
    let diagnostics = harness.diagnostics(&broken).expect("published");
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].source.as_deref(), Some("naga"));
    assert_eq!(diagnostics[0].range.start.line, 1);
}

/// Squiggles that are fixed must be cleared, and clearing means publishing an
/// empty array — not publishing nothing.
#[test]
fn fixing_an_error_clears_the_squiggles() {
    let mut harness = Harness::with_settings(on_type());
    let uri = harness.open("a.wgsl", "fn main() { let x = nope(); }\n");
    assert!(!harness.diagnostics(&uri).unwrap().is_empty());

    harness.change(&uri, "fn main() { let x = 1.0; }\n");
    assert!(harness.diagnostics(&uri).unwrap().is_empty());
}

#[test]
fn validation_on_type_is_off_until_asked_for() {
    let mut harness = Harness::new();
    let uri = harness.open("a.wgsl", "fn main() {}\n");
    harness.change(&uri, "fn main() { let x = nope(); }\n");
    assert!(harness.diagnostics(&uri).is_none(), "should wait for a save");

    harness.save(&uri);
    assert!(!harness.diagnostics(&uri).unwrap().is_empty());
}

/// A `#version 300 es` shader is valid GLSL that naga does not implement.
/// Reporting every line as an error would be worse than reporting nothing.
#[test]
fn a_dialect_naga_cannot_parse_gets_no_squiggles_but_says_why() {
    let source =
        "#version 300 es\nprecision mediump float;\nout vec4 c;\nvoid main() { c = vec4(1.0); }\n";
    let mut harness = Harness::new();
    let uri = harness.open("es.frag", source);
    assert!(harness.diagnostics(&uri).is_none());

    let info: serde_json::Value = harness
        .request("wgsl/shaderInfo", json!({ "textDocument": { "uri": uri.as_str() } }))
        .expect("shader info");
    assert_eq!(info["stage"], "fragment");
    assert!(info["skipped"].as_str().unwrap().contains("300 es"));
    assert_eq!(info["ok"], false);
}

/// naga cannot parse this at all, so the syntax layer is the only thing that
/// can say anything — and it can say exactly what is wrong.
#[test]
fn an_unbalanced_brace_is_reported_by_the_syntax_layer_in_a_dialect_naga_skips() {
    let mut harness = Harness::new();
    let uri = harness.open("es.frag", "#version 300 es\nvoid main() {\n");
    let diagnostics = harness.diagnostics(&uri).expect("published");
    assert_eq!(diagnostics[0].source.as_deref(), Some("wgsl-syntax"));
    assert!(diagnostics[0].message.contains("unclosed"));
}

/// "Validate Current File" has to validate whether or not the settings say to
/// validate on save — otherwise the command does nothing and says nothing.
#[test]
fn the_validate_command_publishes_whatever_the_settings_say() {
    let mut harness = Harness::new();
    let uri = harness.open("a.wgsl", "fn main() {}\n");
    harness.change(&uri, "fn main() { let x = nope(); }\n");
    assert!(harness.diagnostics(&uri).is_none(), "on-type validation is off");

    harness.notify(
        "wgsl/validate",
        json!({ "textDocument": { "uri": uri.as_str() } }),
    );
    assert!(!harness.diagnostics(&uri).unwrap().is_empty());
}

#[test]
fn closing_a_document_clears_its_diagnostics() {
    let mut harness = Harness::new();
    let uri = harness.open("a.wgsl", "fn main() { let x = nope(); }\n");
    assert!(!harness.diagnostics(&uri).unwrap().is_empty());
    harness.close(&uri);
    assert!(harness.diagnostics(&uri).unwrap().is_empty());
}

// ── Hover ──────────────────────────────────────────────────────────────────

#[test]
fn hovering_a_function_shows_its_signature_and_doc_comment() {
    let mut harness = Harness::new();
    let uri = harness.open("a.wgsl", WGSL);
    let hover: Hover =
        harness.at("textDocument/hover", &uri, find(WGSL, "attenuate(camera", 2)).unwrap();
    let text = hover_text(hover);
    assert!(text.contains("fn attenuate(distance: f32, falloff: f32) -> f32"), "{text}");
    assert!(text.contains("Inverse-square falloff."), "{text}");
}

/// A WGSL `let` declares no type. Only naga knows it, and the hover is where
/// that shows up.
#[test]
fn hovering_a_let_binding_shows_the_inferred_type() {
    let mut harness = Harness::new();
    let uri = harness.open("a.wgsl", WGSL);
    let hover: Hover =
        harness.at("textDocument/hover", &uri, find(WGSL, "squared * falloff", 2)).unwrap();
    assert!(hover_text(hover).contains("f32"));
}

#[test]
fn hovering_a_member_shows_its_type_from_the_struct_it_belongs_to() {
    let mut harness = Harness::new();
    let uri = harness.open("a.wgsl", WGSL);
    let hover: Hover =
        harness.at("textDocument/hover", &uri, find(WGSL, "camera.eye.x", 8)).unwrap();
    // naga canonicalises the alias, so the source's `vec3f` comes back as the
    // spelled-out form. Both name the same type.
    let text = hover_text(hover);
    assert!(text.contains("vec3<f32>"), "{text}");
}

#[test]
fn hovering_a_builtin_shows_its_documentation() {
    let mut harness = Harness::new();
    let source = "fn f(a: vec3f, b: vec3f) -> f32 { return dot(a, b); }\n";
    let uri = harness.open("a.wgsl", source);
    let hover: Hover =
        harness.at("textDocument/hover", &uri, find(source, "dot(a, b)", 1)).unwrap();
    let text = hover_text(hover);
    assert!(text.contains("dot(a: vecN<T>, b: vecN<T>) -> T"), "{text}");
    assert!(text.contains("Dot product"), "{text}");
}

#[test]
fn hovering_a_glsl_builtin_variable_names_the_stages_it_belongs_to() {
    let source = "#version 450\nvoid main() { gl_Position = vec4(0.0); }\n";
    let mut harness = Harness::new();
    let uri = harness.open("a.vert", source);
    let hover: Hover =
        harness.at("textDocument/hover", &uri, find(source, "gl_Position", 3)).unwrap();
    assert!(hover_text(hover).contains("Vertex"));
}

/// The whole point of the syntax layer: a file naga cannot parse still hovers.
#[test]
fn hover_works_in_a_file_naga_cannot_parse() {
    let source = "#version 300 es\nfloat helper(float x) { return x; }\nvoid main() { helper(1.0); }\n";
    let mut harness = Harness::new();
    let uri = harness.open("es.frag", source);
    let hover: Hover =
        harness.at("textDocument/hover", &uri, find(source, "helper(1.0)", 2)).unwrap();
    assert!(hover_text(hover).contains("float helper(float x)"));
}

// ── Completion ─────────────────────────────────────────────────────────────

/// Type the `.` into a document that parsed a moment ago, which is what
/// actually happens in an editor — and the only way the server can have a
/// module to type `camera` with, since `camera.` parses nowhere.
fn complete_after_typing(harness: &mut Harness, valid: &str, edited: &str) -> Vec<String> {
    let uri = harness.open("a.wgsl", valid);
    let offset = edited.find('|').expect("fixture has no `|` cursor marker");
    let typed = edited.replacen('|', "", 1);
    harness.change(&uri, &typed);
    let position = support::position_of(&typed, offset);
    labels(harness.at("textDocument/completion", &uri, position).unwrap())
}

#[test]
fn completing_after_a_dot_offers_that_struct_s_members_and_nothing_else() {
    let mut harness = Harness::new();
    let edited = WGSL.replace("camera.eye.x", "camera.|");
    assert_eq!(complete_after_typing(&mut harness, WGSL, &edited), ["view", "eye"]);
}

#[test]
fn completing_after_a_dot_on_a_vector_offers_swizzles() {
    let mut harness = Harness::new();
    let edited = WGSL.replace("camera.eye.x", "camera.eye.|");
    assert_eq!(
        complete_after_typing(&mut harness, WGSL, &edited),
        ["x", "y", "z", "xy", "xyz", "r", "g", "b", "rg", "rgb"]
    );
}

/// With no module ever produced — a file that has never parsed — member
/// completion still offers something: every field name in the file.
#[test]
fn completing_after_a_dot_falls_back_to_the_files_fields() {
    let mut harness = Harness::new();
    let source = "struct A { alpha: f32 }\nstruct B { beta: f32 }\nfn f( { let x = a.|";
    let (uri, position) = harness.open_at("a.wgsl", source);
    assert!(harness.server().document(&uri).unwrap().module().is_none());
    let labels = labels(harness.at("textDocument/completion", &uri, position).unwrap());
    assert_eq!(labels, ["alpha", "beta"]);
}

/// The old provider ignored the cursor entirely. This is the difference.
#[test]
fn completing_a_bare_name_offers_locals_first_then_the_file_then_builtins() {
    let mut harness = Harness::new();
    let source = WGSL.replace("return 1.0 / (squared * falloff);", "return sq|");
    let (uri, position) = harness.open_at("a.wgsl", &source);
    let items = items(harness.at("textDocument/completion", &uri, position).unwrap());

    let sort = |name: &str| {
        items
            .iter()
            .find(|item| item.label == name)
            .unwrap_or_else(|| panic!("no {name} offered"))
            .sort_text
            .clone()
            .unwrap()
    };
    // Locals sort above file-scope names, which sort above the language's own.
    assert!(sort("squared") < sort("attenuate"));
    assert!(sort("attenuate") < sort("dot"));
    assert!(sort("dot") < sort("fn"));
}

#[test]
fn completing_after_an_at_sign_offers_wgsl_attributes() {
    let mut harness = Harness::new();
    let (uri, position) = harness.open_at("a.wgsl", "@|\nfn main() {}\n");
    let labels = labels(harness.at("textDocument/completion", &uri, position).unwrap());
    assert!(labels.contains(&"fragment".to_string()));
    assert!(labels.contains(&"workgroup_size".to_string()));
    // Not a general list: `dot` has no business here.
    assert!(!labels.contains(&"dot".to_string()));
}

#[test]
fn completing_inside_builtin_offers_the_pipeline_values() {
    let mut harness = Harness::new();
    let (uri, position) =
        harness.open_at("a.wgsl", "@vertex\nfn main(@builtin(|) i: u32) {}\n");
    let labels = labels(harness.at("textDocument/completion", &uri, position).unwrap());
    assert!(labels.contains(&"vertex_index".to_string()));
    assert!(labels.contains(&"position".to_string()));
}

#[test]
fn completing_inside_a_var_template_offers_address_spaces() {
    let mut harness = Harness::new();
    let (uri, position) = harness.open_at("a.wgsl", "var<|> thing: f32;\n");
    let labels = labels(harness.at("textDocument/completion", &uri, position).unwrap());
    assert!(labels.contains(&"uniform".to_string()));
    assert!(labels.contains(&"storage".to_string()));
    assert!(labels.contains(&"read_write".to_string()));
}

#[test]
fn completing_after_a_colon_offers_types_only() {
    let mut harness = Harness::new();
    let source = "struct Thing { a: f32 }\nfn main() { var x: | }\n";
    let (uri, position) = harness.open_at("a.wgsl", source);
    let items = items(harness.at("textDocument/completion", &uri, position).unwrap());
    let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
    assert!(labels.contains(&"vec4f"));
    // The file's own struct is a type too.
    assert!(labels.contains(&"Thing"));
    // A function is not.
    assert!(!labels.contains(&"dot"));
}

#[test]
fn completing_after_a_hash_offers_glsl_directives() {
    let mut harness = Harness::new();
    let (uri, position) = harness.open_at("a.frag", "#version 450\n#|\nvoid main() {}\n");
    let labels = labels(harness.at("textDocument/completion", &uri, position).unwrap());
    assert!(labels.contains(&"define".to_string()));
    assert!(labels.contains(&"ifdef".to_string()));
}

#[test]
fn completing_inside_layout_offers_qualifiers() {
    let mut harness = Harness::new();
    let source = "#version 450\nlayout(|) in vec3 position;\nvoid main() {}\n";
    let (uri, position) = harness.open_at("a.vert", source);
    let labels = labels(harness.at("textDocument/completion", &uri, position).unwrap());
    assert!(labels.contains(&"location".to_string()));
    assert!(labels.contains(&"std140".to_string()));
}

#[test]
fn glsl_completion_offers_the_gl_builtins() {
    let mut harness = Harness::new();
    let source = "#version 450\nvoid main() { gl|; }\n";
    let (uri, position) = harness.open_at("a.vert", source);
    let items = items(harness.at("textDocument/completion", &uri, position).unwrap());
    let position_item =
        items.iter().find(|item| item.label == "gl_Position").expect("gl_Position");
    assert_eq!(position_item.kind, Some(CompletionItemKind::VARIABLE));
}

#[test]
fn completion_can_be_switched_off_without_a_restart() {
    let mut harness = Harness::new();
    let (uri, position) = harness.open_at("a.wgsl", "fn main() { d| }\n");
    assert!(harness
        .at::<CompletionResponse>("textDocument/completion", &uri, position)
        .is_some());

    harness.configure(json!({ "wgsl": { "completion": { "enabled": false } } }));
    assert!(harness
        .at::<CompletionResponse>("textDocument/completion", &uri, position)
        .is_none());
}

// ── Definition, references, rename ─────────────────────────────────────────

#[test]
fn go_to_definition_finds_the_declaration_of_a_call() {
    let mut harness = Harness::new();
    let uri = harness.open("a.wgsl", WGSL);
    let response: GotoDefinitionResponse =
        harness.at("textDocument/definition", &uri, find(WGSL, "attenuate(camera", 2)).unwrap();
    let location = &locations(response)[0];
    assert_eq!(location.uri, uri);
    assert_eq!(location.range.start, find(WGSL, "fn attenuate", 3));
}

#[test]
fn go_to_definition_on_a_member_lands_on_the_struct_field() {
    let mut harness = Harness::new();
    let uri = harness.open("a.wgsl", WGSL);
    let response: GotoDefinitionResponse =
        harness.at("textDocument/definition", &uri, find(WGSL, "camera.eye.x", 8)).unwrap();
    assert_eq!(locations(response)[0].range.start, find(WGSL, "eye: vec3f", 0));
}

/// A local shadowing a global must resolve to the local.
#[test]
fn go_to_definition_respects_shadowing() {
    let source = "\
var total: f32 = 0.0;
fn f() -> f32 {
    let total = 1.0;
    return total;
}
";
    let mut harness = Harness::new();
    let uri = harness.open("a.wgsl", source);
    let response: GotoDefinitionResponse =
        harness.at("textDocument/definition", &uri, find(source, "return total", 7)).unwrap();
    assert_eq!(locations(response)[0].range.start, find(source, "let total", 4));
}

#[test]
fn go_to_definition_reaches_a_file_the_editor_has_not_opened() {
    let mut harness = Harness::new();
    harness.workspace_files(&[("lib.wgsl", "fn shared(x: f32) -> f32 { return x; }\n")]);
    let uri = harness.open("a.wgsl", "fn main() { let y = shared(1.0); }\n");

    let response: GotoDefinitionResponse = harness
        .at("textDocument/definition", &uri, Position { line: 0, character: 22 })
        .unwrap();
    assert_eq!(locations(response)[0].uri, uri_for("lib.wgsl"));
}

#[test]
fn references_finds_every_use_and_the_declaration() {
    let mut harness = Harness::new();
    let uri = harness.open("a.wgsl", WGSL);
    let locations: Vec<Location> = harness
        .request(
            "textDocument/references",
            json!({
                "textDocument": { "uri": uri.as_str() },
                "position": find(WGSL, "fn attenuate", 4),
                "context": { "includeDeclaration": true },
            }),
        )
        .unwrap();
    assert_eq!(locations.len(), 2);

    let without: Vec<Location> = harness
        .request(
            "textDocument/references",
            json!({
                "textDocument": { "uri": uri.as_str() },
                "position": find(WGSL, "fn attenuate", 4),
                "context": { "includeDeclaration": false },
            }),
        )
        .unwrap();
    assert_eq!(without.len(), 1);
}

#[test]
fn rename_rewrites_every_occurrence() {
    let mut harness = Harness::new();
    let uri = harness.open("a.wgsl", WGSL);
    let edit: WorkspaceEdit = harness
        .request(
            "textDocument/rename",
            json!({
                "textDocument": { "uri": uri.as_str() },
                "position": find(WGSL, "fn attenuate", 4),
                "newName": "falloffAt",
            }),
        )
        .unwrap();
    let edits: &Vec<TextEdit> = &edit.changes.as_ref().unwrap()[&uri];
    assert_eq!(edits.len(), 2);
    assert!(edits.iter().all(|edit| edit.new_text == "falloffAt"));
}

/// A rename box that produces an invalid program is worse than none.
#[test]
fn renaming_a_builtin_is_refused_before_the_box_opens() {
    let source = "fn f(a: vec3f, b: vec3f) -> f32 { return dot(a, b); }\n";
    let mut harness = Harness::new();
    let uri = harness.open("a.wgsl", source);

    assert!(harness
        .at::<PrepareRenameResponse>(
            "textDocument/prepareRename",
            &uri,
            find(source, "dot(a, b)", 1)
        )
        .is_none());
    // …and a rename *to* a builtin name is refused too.
    assert!(harness
        .request::<WorkspaceEdit>(
            "textDocument/rename",
            json!({
                "textDocument": { "uri": uri.as_str() },
                "position": find(source, "fn f(", 3),
                "newName": "mix",
            }),
        )
        .is_none());
}

// ── Structure ──────────────────────────────────────────────────────────────

#[test]
fn the_outline_is_hierarchical_and_leaves_locals_out() {
    let mut harness = Harness::new();
    let uri = harness.open("a.wgsl", WGSL);
    let response: DocumentSymbolResponse = harness
        .request("textDocument/documentSymbol", json!({ "textDocument": { "uri": uri.as_str() } }))
        .unwrap();
    let DocumentSymbolResponse::Nested(symbols) = response else {
        panic!("expected a nested outline");
    };

    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["Camera", "camera", "AMBIENT", "attenuate", "fs_main"]);

    let camera = &symbols[0];
    let fields: Vec<&str> = children(camera).iter().map(|s| s.name.as_str()).collect();
    assert_eq!(fields, ["view", "eye"]);

    // A function shows its parameters but not its locals.
    let attenuate = &symbols[3];
    let inner: Vec<&str> = children(attenuate).iter().map(|s| s.name.as_str()).collect();
    assert_eq!(inner, ["distance", "falloff"]);
}

fn children(symbol: &DocumentSymbol) -> &[DocumentSymbol] {
    symbol.children.as_deref().unwrap_or(&[])
}

#[test]
fn the_glsl_outline_distinguishes_an_interface_block_from_its_instance() {
    let mut harness = Harness::new();
    let uri = harness.open("a.frag", GLSL);
    let response: DocumentSymbolResponse = harness
        .request("textDocument/documentSymbol", json!({ "textDocument": { "uri": uri.as_str() } }))
        .unwrap();
    let DocumentSymbolResponse::Nested(symbols) = response else {
        panic!("expected a nested outline");
    };
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "MAX_LIGHTS", "Camera", "camera", "position", "fragColour", "Light", "light",
            "AMBIENT", "attenuate", "main"
        ]
    );
    assert_eq!(symbols[1].kind, lsp_types::SymbolKind::INTERFACE);
    assert_eq!(symbols[2].kind, lsp_types::SymbolKind::VARIABLE);
    // `main` is the entry point, and gets its own icon.
    assert_eq!(symbols[9].kind, lsp_types::SymbolKind::METHOD);
}

#[test]
fn workspace_symbols_span_open_and_unopened_files() {
    let mut harness = Harness::new();
    harness.workspace_files(&[("lib.wgsl", "fn sharedHelper() {}\n")]);
    harness.open("a.wgsl", "fn openHelper() {}\n");

    let names = workspace_symbol_names(&mut harness, "helper");
    assert!(names.contains(&"sharedHelper".to_string()));
    assert!(names.contains(&"openHelper".to_string()));
}

/// Opening a file must not make its symbols appear twice.
#[test]
fn an_open_file_supersedes_its_indexed_copy() {
    let mut harness = Harness::new();
    harness.workspace_files(&[("a.wgsl", "fn thing() {}\n")]);
    harness.open("a.wgsl", "fn thing() {}\n");

    assert_eq!(workspace_symbol_names(&mut harness, "thing").len(), 1);
}

/// …and closing it must not lose them.
#[test]
fn closing_a_file_returns_it_to_the_index() {
    let mut harness = Harness::new();
    let uri = harness.open("a.wgsl", "fn thing() {}\n");
    harness.close(&uri);

    assert_eq!(workspace_symbol_names(&mut harness, "thing").len(), 1);
}

/// `WorkspaceSymbolResponse` is untagged, and a `WorkspaceSymbol` carrying a
/// plain `Location` is byte-identical on the wire to the flat
/// `SymbolInformation`. Which variant serde picks says nothing about the
/// server, so read the names out of either.
fn workspace_symbol_names(harness: &mut Harness, query: &str) -> Vec<String> {
    let response: WorkspaceSymbolResponse =
        harness.request("workspace/symbol", json!({ "query": query })).unwrap();
    match response {
        WorkspaceSymbolResponse::Nested(symbols) => {
            symbols.into_iter().map(|symbol| symbol.name).collect()
        }
        WorkspaceSymbolResponse::Flat(symbols) => {
            symbols.into_iter().map(|symbol| symbol.name).collect()
        }
    }
}

#[test]
fn folding_covers_bodies_and_comment_runs_but_not_argument_lists() {
    let source = "\
// one
// two
fn f(
    a: f32,
    b: f32,
) -> f32 {
    return a + b;
}
";
    let mut harness = Harness::new();
    let uri = harness.open("a.wgsl", source);
    let ranges: Vec<FoldingRange> = harness
        .request("textDocument/foldingRange", json!({ "textDocument": { "uri": uri.as_str() } }))
        .unwrap();

    let spans: Vec<(u32, u32)> =
        ranges.iter().map(|range| (range.start_line, range.end_line)).collect();
    // The comment run, and the body — whose `}` stays visible.
    assert!(spans.contains(&(0, 1)), "{spans:?}");
    assert!(spans.contains(&(5, 6)), "{spans:?}");
    // The wrapped parameter list is not a foldable region.
    assert!(!spans.contains(&(2, 5)), "{spans:?}");
}

// ── Signature help ─────────────────────────────────────────────────────────

#[test]
fn signature_help_tracks_which_argument_the_cursor_is_in() {
    let mut harness = Harness::new();
    let source = "fn f() -> f32 { return mix(1.0, 2.0, |0.5); }\n";
    let (uri, position) = harness.open_at("a.wgsl", source);
    let help: SignatureHelp =
        harness.at("textDocument/signatureHelp", &uri, position).unwrap();
    assert_eq!(help.signatures[0].label, "mix(e1: T, e2: T, e3: T) -> T");
    assert_eq!(help.signatures[0].active_parameter, Some(2));
}

#[test]
fn signature_help_works_for_a_function_declared_in_the_file() {
    let mut harness = Harness::new();
    let source = WGSL.replace("attenuate(camera.eye.x, 2.0)", "attenuate(|1.0, 2.0)");
    let (uri, position) = harness.open_at("a.wgsl", &source);
    let help: SignatureHelp =
        harness.at("textDocument/signatureHelp", &uri, position).unwrap();
    assert_eq!(
        help.signatures[0].label,
        "fn attenuate(distance: f32, falloff: f32) -> f32"
    );
    assert_eq!(help.signatures[0].active_parameter, Some(0));
}

// ── Semantic tokens ────────────────────────────────────────────────────────

#[test]
fn semantic_tokens_paint_a_user_type_as_a_struct() {
    let mut harness = Harness::new();
    let source = "struct Light { colour: vec3f }\nvar l: Light;\n";
    let uri = harness.open("a.wgsl", source);
    let result: SemanticTokensResult = harness
        .request(
            "textDocument/semanticTokens/full",
            json!({ "textDocument": { "uri": uri.as_str() } }),
        )
        .unwrap();
    let SemanticTokensResult::Tokens(tokens) = result else {
        panic!("expected a full token array");
    };

    // `Light` appears twice: the declaration and the use in `var l: Light`.
    // Type index 2 is STRUCT; see `semantic_tokens::TYPES`.
    let structs = tokens.data.iter().filter(|token| token.token_type == 2).count();
    assert_eq!(structs, 2);
    // The declaration carries the `declaration` modifier, bit 0.
    assert!(tokens
        .data
        .iter()
        .any(|token| token.token_type == 2 && token.token_modifiers_bitset & 1 == 1));
}

#[test]
fn a_delta_sends_only_what_changed() {
    let mut harness = Harness::new();
    let uri = harness.open("a.wgsl", "fn a() {}\nfn b() {}\nfn c() {}\n");
    let result: SemanticTokensResult = harness
        .request(
            "textDocument/semanticTokens/full",
            json!({ "textDocument": { "uri": uri.as_str() } }),
        )
        .unwrap();
    let SemanticTokensResult::Tokens(first) = result else {
        panic!("expected a full token array");
    };
    let result_id = first.result_id.unwrap();

    harness.change(&uri, "fn a() {}\nfn renamed() {}\nfn c() {}\n");
    let delta: SemanticTokensFullDeltaResult = harness
        .request(
            "textDocument/semanticTokens/full/delta",
            json!({
                "textDocument": { "uri": uri.as_str() },
                "previousResultId": result_id,
            }),
        )
        .unwrap();
    let SemanticTokensFullDeltaResult::TokensDelta(delta) = delta else {
        panic!("expected a delta, got a full resend");
    };
    assert_eq!(delta.edits.len(), 1);
    // One token replaced, and the protocol counts in u32s: five per token.
    assert_eq!(delta.edits[0].delete_count, 5);
}

/// A result id we no longer hold means the client and server have lost sync;
/// a full array resynchronises them.
#[test]
fn an_unknown_result_id_gets_a_full_resend() {
    let mut harness = Harness::new();
    let uri = harness.open("a.wgsl", "fn a() {}\n");
    let result: SemanticTokensFullDeltaResult = harness
        .request(
            "textDocument/semanticTokens/full/delta",
            json!({
                "textDocument": { "uri": uri.as_str() },
                "previousResultId": "nonsense",
            }),
        )
        .unwrap();
    assert!(matches!(result, SemanticTokensFullDeltaResult::Tokens(_)));
}

// ── Inlay hints, code actions, formatting ──────────────────────────────────

#[test]
fn inlay_hints_annotate_a_let_that_declares_no_type() {
    let mut settings = Settings::default();
    settings.wgsl.inlay_hints.enabled = true;
    settings.wgsl.inlay_hints.types = true;

    let mut harness = Harness::with_settings(settings);
    let uri = harness.open("a.wgsl", WGSL);
    let hints: Vec<InlayHint> = harness
        .request(
            "textDocument/inlayHint",
            json!({
                "textDocument": { "uri": uri.as_str() },
                "range": whole(WGSL),
            }),
        )
        .unwrap();

    let labels: Vec<String> = hints
        .iter()
        .map(|hint| match &hint.label {
            InlayHintLabel::String(text) => text.clone(),
            other => panic!("expected a string label, got {other:?}"),
        })
        .collect();
    assert!(labels.contains(&": f32".to_string()), "{labels:?}");
}

#[test]
fn parameter_name_hints_label_the_arguments_of_a_call() {
    let mut settings = Settings::default();
    settings.wgsl.inlay_hints.enabled = true;
    settings.wgsl.inlay_hints.parameter_names = true;

    let source = "fn f() -> f32 { return mix(1.0, 2.0, 0.5); }\n";
    let mut harness = Harness::with_settings(settings);
    let uri = harness.open("a.wgsl", source);
    let hints: Vec<InlayHint> = harness
        .request(
            "textDocument/inlayHint",
            json!({
                "textDocument": { "uri": uri.as_str() },
                "range": whole(source),
            }),
        )
        .unwrap();
    let labels: Vec<String> = hints
        .iter()
        .map(|hint| match &hint.label {
            InlayHintLabel::String(text) => text.clone(),
            other => panic!("expected a string label, got {other:?}"),
        })
        .collect();
    assert_eq!(labels, ["e1:", "e2:", "e3:"]);
}

#[test]
fn inlay_hints_are_silent_until_switched_on() {
    let mut harness = Harness::new();
    let uri = harness.open("a.wgsl", WGSL);
    let hints: Option<Vec<InlayHint>> = harness.request(
        "textDocument/inlayHint",
        json!({ "textDocument": { "uri": uri.as_str() }, "range": whole(WGSL) }),
    );
    assert!(hints.is_none());
}

#[test]
fn a_glsl_file_with_no_version_is_offered_one() {
    let mut harness = Harness::new();
    let uri = harness.open("a.frag", "void main() {}\n");
    let titles = action_titles(&mut harness, &uri, Position { line: 0, character: 0 });
    assert!(titles.iter().any(|title| title.contains("#version 450")), "{titles:?}");
}

/// The stage was guessed from `gl_FragColor`; offer to write the guess down.
#[test]
fn a_guessed_glsl_stage_is_offered_a_pragma() {
    let mut harness = Harness::new();
    let uri = harness.open("a.glsl", "#version 450\nvoid main() { gl_FragColor = vec4(1.0); }\n");
    let titles = action_titles(&mut harness, &uri, Position { line: 0, character: 0 });
    assert!(
        titles.iter().any(|title| title.contains("shader_stage(fragment)")),
        "{titles:?}"
    );

    // A `.frag` file already says which stage it is.
    let uri = harness.open("b.frag", "#version 450\nvoid main() { gl_FragColor = vec4(1.0); }\n");
    let titles = action_titles(&mut harness, &uri, Position { line: 0, character: 0 });
    assert!(!titles.iter().any(|title| title.contains("shader_stage")), "{titles:?}");
}

#[test]
fn a_wgsl_type_can_be_switched_between_its_two_spellings() {
    let mut harness = Harness::new();
    let source = "var a: vec4f;\nvar b: vec4<f32>;\n";
    let uri = harness.open("a.wgsl", source);

    let titles = action_titles(&mut harness, &uri, find(source, "vec4f", 1));
    assert!(titles.iter().any(|title| title.contains("vec4<f32>")), "{titles:?}");

    let titles = action_titles(&mut harness, &uri, find(source, "vec4<f32>", 1));
    assert!(titles.iter().any(|title| title.contains("`vec4f`")), "{titles:?}");
}

fn action_titles(
    harness: &mut Harness,
    uri: &lsp_types::Uri,
    position: Position,
) -> Vec<String> {
    let actions: Vec<serde_json::Value> = harness
        .request(
            "textDocument/codeAction",
            json!({
                "textDocument": { "uri": uri.as_str() },
                "range": { "start": position, "end": position },
                "context": { "diagnostics": [] },
            }),
        )
        .unwrap_or_default();
    actions
        .iter()
        .filter_map(|action| action["title"].as_str().map(str::to_string))
        .collect()
}

#[test]
fn the_formatter_only_touches_leading_whitespace() {
    let mut settings = Settings::default();
    settings.wgsl.format.enable = true;

    let source = "fn f() {\nlet a = 1.0;\n        let b = 2.0;\n}\n";
    let mut harness = Harness::with_settings(settings);
    let uri = harness.open("a.wgsl", source);
    let edits: Vec<TextEdit> = harness
        .request(
            "textDocument/formatting",
            json!({
                "textDocument": { "uri": uri.as_str() },
                "options": { "tabSize": 4, "insertSpaces": true },
            }),
        )
        .unwrap();

    assert_eq!(edits.len(), 2);
    for edit in &edits {
        assert_eq!(edit.new_text, "    ");
        assert_eq!(edit.range.start.character, 0);
    }
    assert_eq!(edits[0].range.start.line, 1);
    assert_eq!(edits[1].range.start.line, 2);
}

#[test]
fn the_formatter_is_off_until_switched_on() {
    let mut harness = Harness::new();
    let uri = harness.open("a.wgsl", "fn f() {\nlet a = 1.0;\n}\n");
    let edits: Option<Vec<TextEdit>> = harness.request(
        "textDocument/formatting",
        json!({
            "textDocument": { "uri": uri.as_str() },
            "options": { "tabSize": 4, "insertSpaces": true },
        }),
    );
    assert!(edits.is_none());
}

// ── Protocol ───────────────────────────────────────────────────────────────

#[test]
fn an_unknown_method_is_method_not_found_rather_than_a_panic() {
    let mut harness = Harness::new();
    let message = harness.error("textDocument/telepathy", json!({})).expect("an error");
    assert!(message.contains("unknown method"));
}

#[test]
fn a_request_for_a_document_that_is_not_open_answers_null() {
    let mut harness = Harness::new();
    let response: Option<Hover> = harness.at(
        "textDocument/hover",
        &uri_for("never-opened.wgsl"),
        Position { line: 0, character: 0 },
    );
    assert!(response.is_none());
}

/// A malformed notification is logged, not fatal.
#[test]
fn a_malformed_notification_is_logged_and_dropped() {
    let mut harness = Harness::new();
    harness.notify("textDocument/didOpen", json!({ "nonsense": true }));
    assert!(harness
        .events()
        .iter()
        .any(|event| event.method == "window/logMessage"));
}

/// Every request must survive a document that is one keystroke from valid,
/// which is the state the editor asks about most of the time.
#[test]
fn every_request_survives_a_half_typed_document() {
    const METHODS: [&str; 8] = [
        "textDocument/completion",
        "textDocument/hover",
        "textDocument/definition",
        "textDocument/documentHighlight",
        "textDocument/signatureHelp",
        "textDocument/prepareRename",
        "textDocument/documentSymbol",
        "textDocument/foldingRange",
    ];

    for partial in [
        "fn",
        "fn f(",
        "fn f(x: ",
        "struct S {",
        "var<uniform> ",
        "fn f() { let a = camera.",
        "@group(0) @binding(",
        "#version",
        "layout(location = 0) in ",
        "float f(float",
    ] {
        for name in ["a.wgsl", "a.frag"] {
            let mut harness = Harness::new();
            let uri = harness.open(name, partial);
            let position = support::position_of(partial, partial.len());
            for method in METHODS {
                // The assertion is that nothing panics and nothing errors;
                // a `null` result is a perfectly good answer here.
                let _: Option<serde_json::Value> = harness.request(
                    method,
                    json!({
                        "textDocument": { "uri": uri.as_str() },
                        "position": position,
                    }),
                );
            }
        }
    }
}

fn on_type() -> Settings {
    let mut settings = Settings::default();
    settings.wgsl.validate.on_type = true;
    settings.glsl.validate.on_type = true;
    settings
}

fn whole(text: &str) -> Range {
    Range {
        start: Position { line: 0, character: 0 },
        end: support::position_of(text, text.len()),
    }
}
