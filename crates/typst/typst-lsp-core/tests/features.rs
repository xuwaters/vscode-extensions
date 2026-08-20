//! P2-17: per-feature tests over fixtures with a cursor marker.
//!
//! Everything here goes through `on_request` / `on_notification`, so the tests
//! exercise the same dispatch path the WASM binding does — a feature that works
//! here works in the editor, modulo transport.

mod support;

use serde_json::{Value, json};
use support::{Harness, at, diagnostics_in, events_named};

// ── Diagnostics ──────────────────────────────────────────────────────────────

#[test]
fn errors_appear_and_then_clear() {
    let mut harness = Harness::new();
    let uri = harness.open("main.typ", "= Fine\n\n#undefined-thing()\n");

    let events = harness.compile(&uri);
    let diagnostics = diagnostics_in(&events, &uri);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    assert_eq!(diagnostics[0]["severity"], json!(1), "expected an error");
    assert!(
        diagnostics[0]["message"].as_str().unwrap().contains("unknown variable"),
        "{}",
        diagnostics[0]["message"]
    );

    harness.change(&uri, "= Fine\n\nAll better.\n");
    let events = harness.compile(&uri);
    let diagnostics = diagnostics_in(&events, &uri);
    assert!(
        diagnostics.is_empty(),
        "a fixed file must be published as an empty array, not left stale"
    );
}

#[test]
fn a_compile_reports_its_status_and_page_count() {
    let mut harness = Harness::new();
    let uri = harness.open("main.typ", "= Title\n\nBody.\n");

    let events = harness.compile(&uri);
    let statuses = events_named(&events, "typst/compileStatus");

    assert_eq!(statuses.first().unwrap()["state"], json!("compiling"));
    let last = statuses.last().unwrap();
    assert_eq!(last["state"], json!("ok"));
    assert_eq!(last["pageCount"], json!(1));
}

#[test]
fn a_broken_document_reports_an_error_status() {
    let mut harness = Harness::new();
    let uri = harness.open("main.typ", "#let x = (1, 2\n");

    let events = harness.compile(&uri);
    let statuses = events_named(&events, "typst/compileStatus");
    assert_eq!(statuses.last().unwrap()["state"], json!("error"));
}

#[test]
fn diagnostics_can_be_switched_off() {
    let mut harness = Harness::new();
    let uri = harness.open("main.typ", "#undefined-thing()\n");

    harness.server().on_notification(
        "workspace/didChangeConfiguration",
        json!({ "settings": { "typstUltra": { "diagnostics": { "enabled": false } } } }),
    );
    harness.server().drain();

    let events = harness.compile(&uri);
    assert!(diagnostics_in(&events, &uri).is_empty());
}

// ── Completion ───────────────────────────────────────────────────────────────

#[test]
fn completion_offers_items_and_replaces_back_past_the_cursor() {
    let mut harness = Harness::new();
    let (uri, position) =
        harness.open_with_cursor("main.typ", "= Doc\n\n#/* CURSOR */\n");

    let result = harness.request("textDocument/completion", at(&uri, position));
    let items = result["items"].as_array().expect("a completion list");
    assert!(items.len() > 50, "expected a rich list, got {}", items.len());

    // Every item must carry a textEdit: typst completions replace back past the
    // cursor, so `insertText` would duplicate the `#`.
    for item in items {
        assert!(
            item.get("textEdit").is_some(),
            "item {:?} has no textEdit",
            item["label"]
        );
        assert!(item.get("insertText").is_none());
    }
}

#[test]
fn completion_after_a_dot_offers_fields() {
    let mut harness = Harness::new();
    let (uri, position) = harness.open_with_cursor(
        "main.typ",
        "#let it = (alpha: 1, beta: 2)\n#it./* CURSOR */\n",
    );

    let result = harness.request("textDocument/completion", at(&uri, position));
    let labels: Vec<String> = result["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["label"].as_str().unwrap_or_default().to_string())
        .collect();

    assert!(labels.iter().any(|label| label == "alpha"), "{labels:?}");
    assert!(labels.iter().any(|label| label == "beta"), "{labels:?}");
}

#[test]
fn completion_preserves_upstreams_relevance_order() {
    let mut harness = Harness::new();
    let (uri, position) = harness.open_with_cursor("main.typ", "#/* CURSOR */\n");

    let result = harness.request("textDocument/completion", at(&uri, position));
    let sorts: Vec<&str> = result["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["sortText"].as_str().unwrap())
        .collect();

    let mut sorted = sorts.clone();
    sorted.sort_unstable();
    assert_eq!(sorts, sorted, "sortText must follow the returned order");
}

// ── Doc comments ─────────────────────────────────────────────────────────────
//
// `tests/fixtures/drawlib` is a cetz-shaped library: doc comments in the
// ecosystem's convention, arguments taken through a sink and read back out of
// a style dictionary. What such a function declares and what it accepts are
// two different lists, and only the second one is any use in an editor.

/// A call into `drawlib`, with the cursor where the marker sits.
fn drawlib(call: &str) -> (Harness, lsp_types::Uri, lsp_types::Position) {
    let mut harness = Harness::new();
    let text = format!("#import \"drawlib/shapes.typ\": *\n#{call}\n");
    let (uri, position) = harness.open_with_cursor("main.typ", &text);
    (harness, uri, position)
}

/// Every completion label, in the order returned.
fn labels(result: &Value) -> Vec<String> {
    result["items"]
        .as_array()
        .expect("a completion list")
        .iter()
        .map(|item| item["label"].as_str().unwrap_or_default().to_string())
        .collect()
}

/// The item with a given label.
fn item<'a>(result: &'a Value, label: &str) -> &'a Value {
    result["items"]
        .as_array()
        .expect("a completion list")
        .iter()
        .find(|item| item["label"] == json!(label))
        .unwrap_or_else(|| panic!("no `{label}` in {:?}", labels(result)))
}

/// The reported bug: `circle` accepts `radius`, and nothing offered it.
#[test]
fn an_argument_a_sink_swallows_is_still_completed() {
    let (mut harness, uri, position) = drawlib("circle((0, 0), /* CURSOR */)");

    let result = harness.request("textDocument/completion", at(&uri, position));
    let labels = labels(&result);
    assert!(labels.contains(&"radius".to_string()), "{labels:?}");

    let radius = item(&result, "radius");
    assert_eq!(radius["textEdit"]["newText"], json!("radius: $1"));
    assert_eq!(radius["detail"], json!("radius: number, array = 1"));
    assert!(
        radius["documentation"]["value"]
            .as_str()
            .unwrap()
            .contains("size of the circle's radius")
    );

    // The declared parameters must still be there, and still first.
    assert_eq!(&labels[..2], ["name", "anchor"], "{labels:?}");
}

/// The docs name the keys worth knowing about; the style dictionary has the
/// rest, and `fill` and `stroke` are always in the rest.
#[test]
fn the_style_root_fills_in_what_the_docs_leave_out() {
    let (mut harness, uri, position) = drawlib("circle((0, 0), /* CURSOR */)");

    let result = harness.request("textDocument/completion", at(&uri, position));
    let labels = labels(&result);
    assert!(labels.contains(&"fill".to_string()), "{labels:?}");
    assert!(labels.contains(&"stroke".to_string()), "{labels:?}");
    assert_eq!(item(&result, "fill")["detail"], json!("fill = auto"));
}

/// `*Root:*` under a `==` heading with no keys of its own: the other spelling
/// the ecosystem uses, and the dictionary is then the only source.
#[test]
fn a_root_with_no_documented_keys_still_resolves() {
    let (mut harness, uri, position) =
        drawlib("ellipse-through((0, 0), (1, 1), /* CURSOR */)");

    let result = harness.request("textDocument/completion", at(&uri, position));
    let labels = labels(&result);
    for expected in ["radius", "stroke", "fill"] {
        assert!(labels.contains(&expected.to_string()), "{labels:?}");
    }
}

#[test]
fn an_argument_already_written_is_not_offered_again() {
    let (mut harness, uri, position) = drawlib("circle((0, 0), radius: 2, /* CURSOR */)");

    let result = harness.request("textDocument/completion", at(&uri, position));
    let labels = labels(&result);
    assert!(!labels.contains(&"radius".to_string()), "{labels:?}");
    assert!(labels.contains(&"fill".to_string()), "{labels:?}");
}

/// Past the colon the cursor is writing a value; a key name there is noise.
#[test]
fn a_value_position_gets_no_argument_names() {
    let (mut harness, uri, position) = drawlib("circle((0, 0), radius: /* CURSOR */)");

    let result = harness.request("textDocument/completion", at(&uri, position));
    assert!(!labels(&result).contains(&"fill".to_string()));
}

/// Nothing is invented for a function that declares everything it takes.
#[test]
fn a_function_without_a_sink_is_left_alone() {
    let (mut harness, uri, position) = drawlib("label-at((0, 0), /* CURSOR */)");

    let result = harness.request("textDocument/completion", at(&uri, position));
    assert_eq!(labels(&result), ["text"]);
}

/// Upstream looks for a comment directly above each parameter, which is not
/// where the convention puts them.
#[test]
fn a_declared_parameter_takes_its_docs_from_the_comment_above_the_function() {
    let (mut harness, uri, position) = drawlib("circle((0, 0), /* CURSOR */)");

    let result = harness.request("textDocument/completion", at(&uri, position));
    let name = item(&result, "name");
    assert_eq!(name["detail"], json!("name: none, str"));
    assert!(
        name["documentation"]["value"]
            .as_str()
            .unwrap()
            .contains("anchor other elements"),
        "{:?}",
        name["documentation"]
    );
}

/// A doc comment says far more than its first sentence, which is all
/// `typst-ide` reads out of one.
#[test]
fn hover_on_a_documented_function_shows_the_whole_comment() {
    let (mut harness, uri, position) = drawlib("cir/* CURSOR */cle((0, 0))");

    let result = harness.request("textDocument/hover", at(&uri, position));
    let value = result["contents"]["value"].as_str().expect("a hover");

    assert!(value.starts_with("Draws a circle or ellipse."), "{value}");
    assert!(value.contains("**Styling**"), "the sections are markdown: {value}");
    assert!(value.contains("- radius (number, array) = 1"), "{value}");
    assert!(value.contains("```typst"), "examples keep their fences: {value}");
}

#[test]
fn hover_on_an_argument_a_sink_swallows_explains_it() {
    let (mut harness, uri, position) = drawlib("circle((0, 0), rad/* CURSOR */ius: 2)");

    let result = harness.request("textDocument/hover", at(&uri, position));
    let value = result["contents"]["value"].as_str().expect("a hover");
    assert!(value.contains("radius: number, array = 1"), "{value}");
    assert!(value.contains("size of the circle's radius"), "{value}");
}

#[test]
fn signature_help_documents_a_closure_from_its_doc_comment() {
    let (mut harness, uri, position) = drawlib("circle((0, 0), /* CURSOR */)");

    let result = harness.request("textDocument/signatureHelp", at(&uri, position));
    let signature = &result["signatures"][0];
    assert_eq!(
        signature["label"],
        json!("circle(..points-style, name = none, anchor = none)")
    );
    assert!(
        signature["documentation"]["value"]
            .as_str()
            .unwrap()
            .contains("Draws a circle or ellipse.")
    );

    let sink = &signature["parameters"][0];
    assert_eq!(sink["label"], json!("..points-style"));
    assert!(
        sink["documentation"]["value"]
            .as_str()
            .unwrap()
            .contains("The position to place the circle on."),
        "{sink:?}"
    );
}

// ── Hover ────────────────────────────────────────────────────────────────────

#[test]
fn hover_describes_a_standard_library_function() {
    let mut harness = Harness::new();
    let (uri, position) =
        harness.open_with_cursor("main.typ", "#lore/* CURSOR */m(10)\n");

    let result = harness.request("textDocument/hover", at(&uri, position));
    let value = result["contents"]["value"].as_str().unwrap_or_default();
    assert!(!value.is_empty(), "expected a tooltip, got {result}");
}

#[test]
fn hover_on_a_reference_names_the_page_it_lands_on() {
    let mut harness = Harness::new();
    // Numbering is required for a heading to be referenceable at all — without
    // it typst refuses the `@intro` and there is no document to ask.
    let text = "#set heading(numbering: \"1.\")\n\n= Intro <intro>\n\n\
                See @in/* CURSOR */tro for details.\n";
    let (uri, position) = harness.open_with_cursor("main.typ", text);
    harness.compile(&uri);

    let result = harness.request("textDocument/hover", at(&uri, position));
    let value = result["contents"]["value"].as_str().unwrap_or_default();
    assert!(
        value.contains("On page 1."),
        "expected a page number in the hover, got {value:?}"
    );
}

#[test]
fn hover_on_whitespace_answers_nothing() {
    let mut harness = Harness::new();
    let (uri, position) = harness.open_with_cursor("main.typ", "= A\n\n/* CURSOR */\n");

    let result = harness.request("textDocument/hover", at(&uri, position));
    assert_eq!(result, json!(null));
}

// ── Definition ───────────────────────────────────────────────────────────────

#[test]
fn definition_jumps_to_a_local_binding() {
    let mut harness = Harness::new();
    let text = "#let helper(x) = x + 1\n\n#hel/* CURSOR */per(2)\n";
    let (uri, position) = harness.open_with_cursor("main.typ", text);

    let result = harness.request("textDocument/definition", at(&uri, position));
    assert_eq!(result["uri"], json!(uri.as_str()));
    assert_eq!(result["range"]["start"]["line"], json!(0));
}

#[test]
fn definition_of_a_standard_library_item_answers_null() {
    let mut harness = Harness::new();
    let (uri, position) = harness.open_with_cursor("main.typ", "#lore/* CURSOR */m(5)\n");

    let result = harness.request("textDocument/definition", at(&uri, position));
    assert_eq!(
        result,
        json!(null),
        "there is no source location for a built-in; hover carries the docs instead"
    );
}

#[test]
fn definition_of_an_import_jumps_to_the_file() {
    let mut harness = Harness::new();
    let text = "#import \"helper.typ\"/* CURSOR */: greet\n";
    let (uri, position) = harness.open_with_cursor("main.typ", text);

    let result = harness.request("textDocument/definition", at(&uri, position));
    assert!(
        result["uri"].as_str().unwrap_or_default().ends_with("helper.typ"),
        "got {result}"
    );
}

#[test]
fn definition_of_a_bibliography_path_jumps_to_the_file() {
    let mut harness = Harness::new();
    harness.open("refs.bib", REFS);
    let (uri, position) =
        harness.open_with_cursor("main.typ", "#bibliography(\"re/* CURSOR */fs.bib\")\n");

    let result = harness.request("textDocument/definition", at(&uri, position));
    assert!(
        result["uri"].as_str().unwrap_or_default().ends_with("refs.bib"),
        "got {result}"
    );
    assert_eq!(result["range"]["start"]["line"], json!(0));
}

#[test]
fn definition_of_a_path_that_is_not_there_answers_null() {
    let mut harness = Harness::new();
    let (uri, position) =
        harness.open_with_cursor("main.typ", "#image(\"mis/* CURSOR */sing.png\")\n");

    let result = harness.request("textDocument/definition", at(&uri, position));
    assert_eq!(result, json!(null), "a path that resolves nowhere is no jump");
}

#[test]
fn definition_of_a_reference_jumps_to_the_label_that_declares_it() {
    let mut harness = Harness::new();
    let text = "= Intro <intro>\n\nSee @in/* CURSOR */tro.\n";
    let (uri, position) = harness.open_with_cursor("main.typ", text);

    let result = harness.request("textDocument/definition", at(&uri, position));
    assert_eq!(result["uri"], json!(uri.as_str()));
    assert_eq!(result["range"]["start"], json!({ "line": 0, "character": 8 }));
    assert_eq!(result["range"]["end"], json!({ "line": 0, "character": 15 }));
}

#[test]
fn definition_of_a_label_used_as_a_value_jumps_to_its_declaration() {
    let mut harness = Harness::new();
    let text = "= Intro <intro>\n\n#context counter(heading).at(<in/* CURSOR */tro>)\n";
    let (uri, position) = harness.open_with_cursor("main.typ", text);

    let result = harness.request("textDocument/definition", at(&uri, position));
    assert_eq!(
        result["range"]["start"],
        json!({ "line": 0, "character": 8 }),
        "`<intro>` in code names the label markup declares: {result:#}"
    );
}

#[test]
fn definition_of_a_label_reaches_across_the_compile_graph() {
    let mut harness = Harness::new();
    harness.open("chapter.typ", "= Intro <intro>\n");
    let text = "#include \"chapter.typ\"\n\n#context counter(heading).at(<in/* CURSOR */tro>)\n";
    let (uri, position) = harness.open_with_cursor("main.typ", text);
    harness.compile(&uri);

    let result = harness.request("textDocument/definition", at(&uri, position));
    assert!(
        result["uri"].as_str().unwrap_or_default().ends_with("chapter.typ"),
        "got {result}"
    );
}

#[test]
fn definition_of_a_label_answers_without_a_compiled_document() {
    let mut harness = Harness::new();
    // `#lorem()` takes an integer; this document does not compile, which is the
    // normal state of one being edited.
    let text = "= Intro <intro>\n\n#lorem(\"x\")\n\nSee @in/* CURSOR */tro.\n";
    let (uri, position) = harness.open_with_cursor("main.typ", text);
    harness.compile(&uri);

    let result = harness.request("textDocument/definition", at(&uri, position));
    assert_eq!(result["range"]["start"]["line"], json!(0), "got {result}");
}

#[test]
fn definition_standing_on_a_label_declaration_answers_null() {
    let mut harness = Harness::new();
    let (uri, position) = harness.open_with_cursor("main.typ", "= Intro <in/* CURSOR */tro>\n");

    let result = harness.request("textDocument/definition", at(&uri, position));
    assert_eq!(result, json!(null), "the declaration is where a jump would land");
}

// ── References and rename ────────────────────────────────────────────────────

#[test]
fn references_finds_every_use_of_a_label() {
    let mut harness = Harness::new();
    let text = "= Intro <in/* CURSOR */tro>\n\nSee @intro and again @intro.\n";
    let (uri, position) = harness.open_with_cursor("main.typ", text);
    harness.compile(&uri);

    let mut params = at(&uri, position);
    params["context"] = json!({ "includeDeclaration": true });
    let result = harness.request("textDocument/references", params);

    let locations = result.as_array().expect("a location list");
    assert_eq!(locations.len(), 3, "declaration plus two references: {locations:#?}");
}

#[test]
fn references_respects_include_declaration() {
    let mut harness = Harness::new();
    let text = "= Intro <in/* CURSOR */tro>\n\nSee @intro.\n";
    let (uri, position) = harness.open_with_cursor("main.typ", text);

    let mut params = at(&uri, position);
    params["context"] = json!({ "includeDeclaration": false });
    let result = harness.request("textDocument/references", params);

    assert_eq!(result.as_array().unwrap().len(), 1);
}

#[test]
fn references_counts_a_label_used_in_code_as_a_use_not_a_declaration() {
    let mut harness = Harness::new();
    let text =
        "= Intro <in/* CURSOR */tro>\n\nSee @intro.\n\n#context counter(heading).at(<intro>)\n";
    let (uri, position) = harness.open_with_cursor("main.typ", text);

    let mut params = at(&uri, position);
    params["context"] = json!({ "includeDeclaration": false });
    let result = harness.request("textDocument/references", params);

    let locations = result.as_array().expect("a location list");
    assert_eq!(
        locations.len(),
        2,
        "`@intro` and the `<intro>` argument, but not the declaration: {locations:#?}"
    );
    assert_eq!(locations[0]["range"]["start"]["line"], json!(2));
    assert_eq!(locations[1]["range"]["start"]["line"], json!(4));
}

#[test]
fn references_from_a_label_used_in_code_finds_the_whole_set() {
    let mut harness = Harness::new();
    let text =
        "= Intro <intro>\n\nSee @intro.\n\n#context counter(heading).at(<in/* CURSOR */tro>)\n";
    let (uri, position) = harness.open_with_cursor("main.typ", text);

    let mut params = at(&uri, position);
    params["context"] = json!({ "includeDeclaration": true });
    let result = harness.request("textDocument/references", params);

    assert_eq!(result.as_array().unwrap().len(), 3, "{result:#?}");
}

#[test]
fn references_finds_uses_of_a_local_binding() {
    let mut harness = Harness::new();
    let text = "#let val/* CURSOR */ue = 1\n\n#value #value\n";
    let (uri, position) = harness.open_with_cursor("main.typ", text);

    let mut params = at(&uri, position);
    params["context"] = json!({ "includeDeclaration": true });
    let result = harness.request("textDocument/references", params);

    assert_eq!(result.as_array().unwrap().len(), 3, "{result:#?}");
}

#[test]
fn rename_rewrites_a_label_and_its_references_without_the_delimiters() {
    let mut harness = Harness::new();
    let text = "= Intro <in/* CURSOR */tro>\n\nSee @intro.\n";
    let (uri, position) = harness.open_with_cursor("main.typ", text);

    let mut params = at(&uri, position);
    params["newName"] = json!("overview");
    let result = harness.request("textDocument/rename", params);

    let edits = result["changes"][uri.as_str()].as_array().expect("edits");
    assert_eq!(edits.len(), 2);
    for edit in edits {
        assert_eq!(edit["newText"], json!("overview"));
        // The edit must cover only the name, so `<` and `>` survive.
        let start = edit["range"]["start"]["character"].as_u64().unwrap();
        let end = edit["range"]["end"]["character"].as_u64().unwrap();
        assert_eq!(end - start, "intro".len() as u64, "{edit}");
    }
}

#[test]
fn rename_refuses_a_standard_library_item_with_a_reason() {
    let mut harness = Harness::new();
    let (uri, position) = harness.open_with_cursor("main.typ", "#lore/* CURSOR */m(5)\n");

    let mut params = at(&uri, position);
    params["newName"] = json!("ipsum");
    let message = harness.request_err("textDocument/rename", params);

    assert!(
        message.contains("standard library"),
        "the refusal must say why: {message}"
    );
}

#[test]
fn prepare_rename_declines_where_rename_would_refuse() {
    let mut harness = Harness::new();
    let (uri, position) = harness.open_with_cursor("main.typ", "#lore/* CURSOR */m(5)\n");

    let result = harness.request("textDocument/prepareRename", at(&uri, position));
    assert_eq!(result, json!(null));
}

#[test]
fn prepare_rename_offers_the_name_only() {
    let mut harness = Harness::new();
    let text = "= Intro <in/* CURSOR */tro>\n";
    let (uri, position) = harness.open_with_cursor("main.typ", text);

    let result = harness.request("textDocument/prepareRename", at(&uri, position));
    let start = result["start"]["character"].as_u64().unwrap();
    let end = result["end"]["character"].as_u64().unwrap();
    assert_eq!(end - start, "intro".len() as u64, "got {result}");
}

// ── Symbols ──────────────────────────────────────────────────────────────────

#[test]
fn document_symbols_come_back_nested() {
    let mut harness = Harness::new();
    let uri = harness.open(
        "main.typ",
        "= One\n\n#let helper = 1\n\n== One A\n\n= Two\n",
    );

    let result = harness.request(
        "textDocument/documentSymbol",
        json!({ "textDocument": { "uri": uri.as_str() } }),
    );

    let roots = result.as_array().unwrap();
    assert_eq!(roots.len(), 2);
    assert_eq!(roots[0]["name"], json!("One"));
    let children = roots[0]["children"].as_array().unwrap();
    assert!(children.iter().any(|child| child["name"] == json!("helper")));
    assert!(children.iter().any(|child| child["name"] == json!("One A")));
}

#[test]
fn workspace_symbols_search_the_files_the_host_reported() {
    let mut harness = Harness::new();
    let helper = harness.uri("helper.typ");
    harness.server().on_notification(
        "typst/workspaceFiles",
        json!({ "uris": [helper.as_str()] }),
    );

    let result = harness.request("workspace/symbol", json!({ "query": "greet" }));
    let symbols = result.as_array().unwrap();
    assert!(
        symbols.iter().any(|symbol| symbol["name"] == json!("greet")),
        "{symbols:#?}"
    );
}

// ── Semantic tokens ──────────────────────────────────────────────────────────

#[test]
fn semantic_tokens_are_produced_and_then_delta_encoded() {
    let mut harness = Harness::new();
    let uri = harness.open("main.typ", "= Heading\n\n*bold* and `raw`.\n");

    let full = harness.request(
        "textDocument/semanticTokens/full",
        json!({ "textDocument": { "uri": uri.as_str() } }),
    );
    let result_id = full["resultId"].as_str().unwrap().to_string();
    assert!(!full["data"].as_array().unwrap().is_empty());

    harness.change(&uri, "= Heading\n\n*bold* and `raw` and _emph_.\n");
    let delta = harness.request(
        "textDocument/semanticTokens/full/delta",
        json!({
            "textDocument": { "uri": uri.as_str() },
            "previousResultId": result_id,
        }),
    );

    assert!(delta.get("edits").is_some(), "expected a delta, got {delta}");
}

#[test]
fn an_unknown_result_id_gets_the_whole_array_back() {
    let mut harness = Harness::new();
    let uri = harness.open("main.typ", "= Heading\n");

    let result = harness.request(
        "textDocument/semanticTokens/full/delta",
        json!({
            "textDocument": { "uri": uri.as_str() },
            "previousResultId": "not-a-real-id",
        }),
    );

    assert!(result.get("data").is_some(), "expected full tokens, got {result}");
}

// ── Folding, selection, links ────────────────────────────────────────────────

#[test]
fn folding_ranges_cover_headings_and_blocks() {
    let mut harness = Harness::new();
    let uri = harness.open("main.typ", "= One\n\nbody\n\n= Two\n\n#{\n  1\n}\n");

    let result = harness.request(
        "textDocument/foldingRange",
        json!({ "textDocument": { "uri": uri.as_str() } }),
    );
    assert!(result.as_array().unwrap().len() >= 2, "{result}");
}

#[test]
fn selection_ranges_nest_outwards() {
    let mut harness = Harness::new();
    let text = "#let f(x) = x/* CURSOR */ + 1\n";
    let (uri, position) = harness.open_with_cursor("main.typ", text);

    let result = harness.request(
        "textDocument/selectionRange",
        json!({
            "textDocument": { "uri": uri.as_str() },
            "positions": [{ "line": position.line, "character": position.character }],
        }),
    );

    assert!(result[0]["parent"].is_object(), "expected an ancestor chain");
}

#[test]
fn document_links_are_offered_only_when_the_target_resolves() {
    let mut harness = Harness::new();
    let uri = harness.open(
        "main.typ",
        "#import \"helper.typ\": greet\n#import \"nope.typ\": missing\n",
    );

    let result = harness.request(
        "textDocument/documentLink",
        json!({ "textDocument": { "uri": uri.as_str() } }),
    );

    let links = result.as_array().unwrap();
    assert_eq!(links.len(), 1, "only the resolvable import: {links:#?}");
    assert!(links[0]["target"].as_str().unwrap().ends_with("helper.typ"));
}

#[test]
fn a_bare_url_becomes_a_link() {
    let mut harness = Harness::new();
    let uri = harness.open("main.typ", "Visit https://typst.app for docs.\n");

    let result = harness.request(
        "textDocument/documentLink",
        json!({ "textDocument": { "uri": uri.as_str() } }),
    );

    let links = result.as_array().unwrap();
    assert_eq!(links.len(), 1, "{links:#?}");
    assert_eq!(links[0]["target"], json!("https://typst.app"));
}

// ── Formatting ───────────────────────────────────────────────────────────────

#[test]
fn formatting_rewrites_the_whole_document() {
    let mut harness = Harness::new();
    let uri = harness.open("main.typ", "#let   f(x)  =  x+1\n");

    let result = harness.request(
        "textDocument/formatting",
        json!({
            "textDocument": { "uri": uri.as_str() },
            "options": { "tabSize": 2, "insertSpaces": true },
        }),
    );

    let edits = result.as_array().unwrap();
    assert_eq!(edits.len(), 1);
    assert!(edits[0]["newText"].as_str().unwrap().contains("#let f(x) = x + 1"));
}

#[test]
fn formatting_a_broken_document_answers_null() {
    let mut harness = Harness::new();
    let uri = harness.open("main.typ", "#let x = (1, 2\n");

    let result = harness.request(
        "textDocument/formatting",
        json!({
            "textDocument": { "uri": uri.as_str() },
            "options": { "tabSize": 2, "insertSpaces": true },
        }),
    );

    assert_eq!(result, json!(null), "a broken document must not be mangled");
}

#[test]
fn formatting_can_be_switched_off() {
    let mut harness = Harness::new();
    let uri = harness.open("main.typ", "#let   f(x)  =  x+1\n");

    harness.server().on_notification(
        "workspace/didChangeConfiguration",
        json!({ "settings": { "typstUltra": { "formatter": { "mode": "off" } } } }),
    );
    harness.server().drain();

    let result = harness.request(
        "textDocument/formatting",
        json!({
            "textDocument": { "uri": uri.as_str() },
            "options": { "tabSize": 2, "insertSpaces": true },
        }),
    );
    assert_eq!(result, json!(null));
}

// ── Phase 4 features ─────────────────────────────────────────────────────────

#[test]
fn inlay_hints_are_off_until_enabled() {
    let mut harness = Harness::new();
    let uri = harness.open("main.typ", "#rect(10pt, 20pt)\n");

    let params = json!({
        "textDocument": { "uri": uri.as_str() },
        "range": {
            "start": { "line": 0, "character": 0 },
            "end": { "line": 1, "character": 0 },
        },
    });

    assert_eq!(harness.request("textDocument/inlayHint", params.clone()), json!(null));

    harness.server().on_notification(
        "workspace/didChangeConfiguration",
        json!({ "settings": { "typstUltra": { "inlayHints": { "enabled": true } } } }),
    );
    harness.server().drain();

    let hints = harness.request("textDocument/inlayHint", params);
    let hints = hints.as_array().unwrap();
    assert!(!hints.is_empty(), "expected parameter hints once enabled");
    assert!(hints[0]["label"].as_str().unwrap().ends_with(':'));
}

#[test]
fn signature_help_shows_declared_parameters() {
    let mut harness = Harness::new();
    let (uri, position) = harness.open_with_cursor("main.typ", "#rect(/* CURSOR */)\n");

    let result = harness.request("textDocument/signatureHelp", at(&uri, position));
    let label = result["signatures"][0]["label"].as_str().unwrap();
    assert!(label.starts_with("rect("), "got {label}");
    assert!(label.contains("width"), "got {label}");
}

#[test]
fn a_heading_gets_an_add_label_action() {
    let mut harness = Harness::new();
    let uri = harness.open("main.typ", "= My Great Section\n\nBody.\n");

    let result = harness.request(
        "textDocument/codeAction",
        json!({
            "textDocument": { "uri": uri.as_str() },
            "range": {
                "start": { "line": 0, "character": 3 },
                "end": { "line": 0, "character": 3 },
            },
            "context": { "diagnostics": [] },
        }),
    );

    let actions = result.as_array().unwrap();
    let label_action = actions
        .iter()
        .find(|action| action["title"].as_str().unwrap_or_default().contains("Add label"))
        .expect("expected an add-label action");

    assert!(
        label_action["title"].as_str().unwrap().contains("my-great-section"),
        "{label_action}"
    );
}

#[test]
fn code_lenses_offer_preview_and_export() {
    let mut harness = Harness::new();
    let uri = harness.open("main.typ", "= Doc\n");

    let result = harness.request(
        "textDocument/codeLens",
        json!({ "textDocument": { "uri": uri.as_str() } }),
    );

    let commands: Vec<&str> = result
        .as_array()
        .unwrap()
        .iter()
        .map(|lens| lens["command"]["command"].as_str().unwrap())
        .collect();

    assert!(commands.contains(&"typstUltra.showPreviewToSide"));
    assert!(commands.contains(&"typstUltra.export"));
}

// ── Preview and export ───────────────────────────────────────────────────────

#[test]
fn document_metrics_describe_every_page() {
    let mut harness = Harness::new();
    let uri = harness.open("main.typ", "= One\n#pagebreak()\n= Two\n");
    harness.compile(&uri);

    let result = harness
        .request("typst/documentMetrics", json!({ "uri": uri.as_str() }));

    assert_eq!(result["pageCount"], json!(2));
    let pages = result["pages"].as_array().unwrap();
    assert_eq!(pages.len(), 2);
    assert!(pages[0]["widthPt"].as_f64().unwrap() > 100.0);
    assert_eq!(pages[0]["hash"].as_str().unwrap().len(), 16, "16 hex digits");
}

#[test]
fn render_pages_ships_a_page_once_and_then_nothing() {
    let mut harness = Harness::new();
    let uri = harness.open("main.typ", "= One\n\nSome text.\n");
    harness.compile(&uri);

    let first = harness.request(
        "typst/renderPages",
        json!({ "uri": uri.as_str(), "pages": [0], "knownHashes": {} }),
    );
    let patch = &first["patches"][0];
    assert_eq!(patch["op"], json!("replace"));
    assert_eq!(patch["format"], json!("svg"), "svg is the default mode");
    assert!(patch["content"].as_str().unwrap().starts_with("<svg"));

    let hash = patch["hash"].as_str().unwrap();
    let second = harness.request(
        "typst/renderPages",
        json!({
            "uri": uri.as_str(),
            "pages": [0],
            "knownHashes": { "0": hash },
        }),
    );
    assert_eq!(second["patches"][0]["op"], json!("unchanged"));
    assert!(second["patches"][0].get("content").is_none());
}

#[test]
fn render_pages_honours_the_png_mode() {
    let mut harness = Harness::new();
    let uri = harness.open("main.typ", "= One\n\nSome text.\n");
    harness.compile(&uri);

    let result = harness.request(
        "typst/renderPages",
        json!({
            "uri": uri.as_str(),
            "pages": [0],
            "knownHashes": {},
            "mode": "png",
            "ppi": 96.0,
        }),
    );

    let patch = &result["patches"][0];
    assert_eq!(patch["format"], json!("png"));
    assert!(
        patch["content"].as_str().unwrap().starts_with("iVBORw0KGgo"),
        "expected base64 PNG"
    );
}

#[test]
fn jump_from_cursor_and_back_agree() {
    let mut harness = Harness::new();
    let text = "= Heading\n\nThe quick brown/* CURSOR */ fox.\n";
    let (uri, position) = harness.open_with_cursor("main.typ", text);
    harness.compile(&uri);

    let positions = harness.request(
        "typst/jumpFromCursor",
        json!({
            "uri": uri.as_str(),
            "position": { "line": position.line, "character": position.character },
        }),
    );
    let point = &positions.as_array().unwrap()[0];

    let back = harness.request(
        "typst/jumpFromClick",
        json!({
            "page": point["page"],
            "xPt": point["xPt"],
            "yPt": point["yPt"],
        }),
    );

    assert_eq!(back["kind"], json!("source"));
    assert_eq!(back["uri"], json!(uri.as_str()));
}

#[test]
fn export_produces_a_pdf() {
    let mut harness = Harness::new();
    let uri = harness.open("main.typ", "= Title\n\nBody.\n");
    harness.compile(&uri);

    let result = harness.request("typst/export", json!({ "format": "pdf" }));
    assert_eq!(result["extension"], json!("pdf"));

    let encoded = result["files"][0].as_str().unwrap();
    assert!(encoded.starts_with("JVBER"), "base64 of %PDF-, got {}", &encoded[..8]);
}

#[test]
fn export_before_a_successful_compile_says_so() {
    let mut harness = Harness::new();
    harness.open("main.typ", "= Title\n");

    let message = harness.request_err("typst/export", json!({ "format": "pdf" }));
    assert!(message.contains("compile"), "got {message}");
}

// ── Dispatch ─────────────────────────────────────────────────────────────────

#[test]
fn an_unknown_method_is_reported_as_such() {
    let mut harness = Harness::new();
    let message = harness.request_err("textDocument/nonsense", json!({}));
    assert!(message.contains("unknown method"), "got {message}");
}

#[test]
fn a_malformed_notification_logs_instead_of_crashing() {
    let mut harness = Harness::new();
    harness.server().on_notification("textDocument/didOpen", json!({ "wrong": true }));

    let events = harness.server().drain();
    assert!(
        events.iter().any(|event| event.method == "window/logMessage"),
        "a bad payload should be logged, not fatal"
    );
}

#[test]
fn a_file_outside_the_compile_root_is_ignored_rather_than_mis_attributed() {
    let mut harness = Harness::new();
    harness.server().on_notification(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": "file:///somewhere/else/other.typ",
                "languageId": "typst",
                "version": 1,
                "text": "= Outside\n",
            }
        }),
    );

    // No panic, no diagnostics, no state — decision 0008's "not in project".
    assert!(harness.server().drain().is_empty());
}

// ── Untitled buffers ─────────────────────────────────────────────────────────

#[test]
fn an_untitled_buffer_becomes_the_compile_root_and_produces_pages() {
    let mut harness = Harness::new();
    let uri = harness.open_untitled("Untitled-1", "= Draft\n#pagebreak()\n= More\n");

    let events = harness.compile(&uri);
    let statuses = events_named(&events, "typst/compileStatus");
    assert_eq!(statuses.last().unwrap()["state"], json!("ok"));

    let result = harness.request("typst/documentMetrics", json!({ "uri": uri.as_str() }));
    assert_eq!(result["pageCount"], json!(2));
}

#[test]
fn an_untitled_buffers_diagnostics_come_back_addressed_to_it() {
    let mut harness = Harness::new();
    let uri = harness.open_untitled("Untitled-1", "#undefined-thing()\n");

    let events = harness.compile(&uri);
    let diagnostics = diagnostics_in(&events, &uri);
    assert_eq!(diagnostics.len(), 1, "got {diagnostics:#?}");

    // And they come back down again, rather than sticking to a buffer that has
    // no file for the reader to open and fix.
    harness.change(&uri, "= Fine now\n");
    let events = harness.compile(&uri);
    assert!(diagnostics_in(&events, &uri).is_empty());
}

#[test]
fn an_untitled_buffer_is_edited_in_place_like_any_other() {
    let mut harness = Harness::new();
    let uri = harness.open_untitled("Untitled-1", "= One\n");
    harness.compile(&uri);

    harness.change(&uri, "= One\n#pagebreak()\n= Two\n");
    let events = harness.compile(&uri);
    let statuses = events_named(&events, "typst/compileStatus");
    assert_eq!(statuses.last().unwrap()["pageCount"], json!(2));
}

#[test]
fn a_package_import_still_resolves_from_an_untitled_buffer() {
    // The buffer's own path is fictional, but a package path is absolute, so
    // the one kind of import a scratch document is likely to reach for keeps
    // working. Relative imports do not, and cannot: there is no directory.
    let mut harness = Harness::new();
    let uri = harness.open_untitled("Untitled-1", "#import \"nowhere.typ\": *\n");

    let events = harness.compile(&uri);
    let diagnostics = diagnostics_in(&events, &uri);
    assert_eq!(diagnostics.len(), 1, "got {diagnostics:#?}");
    let message = diagnostics[0]["message"].as_str().unwrap();
    assert!(message.contains("file not found"), "got {message}");
}

// ── P4-07 and P4-13 ──────────────────────────────────────────────────────────

#[test]
fn postfix_items_are_offered_after_a_dot_and_sorted_last() {
    let mut harness = Harness::new();
    let text = "#let value = [x]\n\n#value./* CURSOR */\n";
    let (uri, position) = harness.open_with_cursor("main.typ", text);

    let result = harness.request("textDocument/completion", at(&uri, position));
    let items = result["items"].as_array().unwrap();

    let postfix: Vec<&serde_json::Value> = items
        .iter()
        .filter(|item| item["sortText"].as_str().is_some_and(|s| s.starts_with('z')))
        .collect();

    assert!(!postfix.is_empty(), "expected postfix items: {items:#?}");

    let rect = postfix
        .iter()
        .find(|item| item["label"] == json!("rect"))
        .expect("expected a `rect` postfix item");
    assert_eq!(rect["textEdit"]["newText"], json!("rect(value)"));

    // Upstream's field completions must still come first.
    let upstream = items
        .iter()
        .filter(|item| item["sortText"].as_str().is_some_and(|s| !s.starts_with('z')))
        .count();
    assert!(upstream > 0, "upstream's own field completions went missing");
}

#[test]
fn postfix_items_are_not_offered_in_plain_markup() {
    let mut harness = Harness::new();
    let (uri, position) =
        harness.open_with_cursor("main.typ", "Some ordinary text./* CURSOR */\n");

    let result = harness.request("textDocument/completion", at(&uri, position));
    let items = result["items"].as_array().cloned().unwrap_or_default();

    assert!(
        !items
            .iter()
            .any(|item| item["sortText"].as_str().is_some_and(|s| s.starts_with('z'))),
        "a full stop in prose is not a field access"
    );
}

// ── BibTeX ───────────────────────────────────────────────────────────────────

/// The fixture bibliography, as an editor would have it open.
const REFS: &str = include_str!("fixtures/refs.bib");

#[test]
fn a_bibliography_is_checked_on_the_edit_and_cleared_when_fixed() {
    let mut harness = Harness::new();
    let uri = harness.open("refs.bib", "@misc{same, title={A}}\n@misc{same, title={B}}\n");

    let diagnostics = diagnostics_in(&harness.server().drain(), &uri);
    let duplicate = diagnostics
        .iter()
        .find(|d| d["message"].as_str().unwrap_or_default().contains("duplicate"))
        .unwrap_or_else(|| panic!("expected a duplicate-key error: {diagnostics:#?}"));
    assert_eq!(duplicate["severity"], json!(1));
    assert_eq!(duplicate["source"], json!("bibtex"));
    assert!(
        duplicate["relatedInformation"][0]["location"]["uri"] == json!(uri.as_str()),
        "the error should point at the first definition too: {duplicate:#?}"
    );

    harness.change(&uri, "@misc{one, title={A}}\n@misc{two, title={B}}\n");
    let diagnostics = diagnostics_in(&harness.server().drain(), &uri);
    assert!(
        diagnostics.is_empty(),
        "a fixed bibliography must be published as an empty array: {diagnostics:#?}"
    );
}

#[test]
fn a_missing_required_field_is_a_warning_not_an_error() {
    let mut harness = Harness::new();
    let uri = harness.open("refs.bib", "@article{k,\n  title = {T},\n}\n");

    let diagnostics = diagnostics_in(&harness.server().drain(), &uri);
    assert!(!diagnostics.is_empty(), "expected the missing fields to be reported");
    assert!(
        diagnostics.iter().all(|d| d["severity"] == json!(2)),
        "an incomplete entry is a warning, not an error: {diagnostics:#?}"
    );
}

/// The trap this whole feature is built around: a `.bib` file must never become
/// the compile root, or the compiler is handed BibTeX and asked for a document.
#[test]
fn opening_a_bibliography_leaves_the_compile_root_alone() {
    let mut harness = Harness::new();
    let main = harness.open("main.typ", "= Title\n\nBody.\n");
    let bib = harness.open("refs.bib", REFS);

    // The host debounces off the last edited document, which is now the `.bib`.
    let events = harness.compile(&bib);
    let statuses = events_named(&events, "typst/compileStatus");
    assert_eq!(
        statuses.last().unwrap()["state"],
        json!("ok"),
        "the typst document should still be what compiled"
    );
    assert!(diagnostics_in(&events, &main).is_empty());
    assert!(
        diagnostics_in(&events, &bib)
            .iter()
            .all(|d| d["source"] == json!("bibtex")),
        "typst must not be publishing into a `.bib` file"
    );
}

/// The overlay is what the compiler reads, so a citation resolves against the
/// bibliography **as it is being typed** rather than as it was last saved.
#[test]
fn an_unsaved_bibliography_edit_reaches_the_compiler() {
    let mut harness = Harness::new();
    let bib = harness.open("refs.bib", REFS);
    let main = harness.open(
        "main.typ",
        "#bibliography(\"refs.bib\")\n\nSee @knuth1984.\n",
    );

    let events = harness.compile(&main);
    let diagnostics = diagnostics_in(&events, &main);
    assert!(diagnostics.is_empty(), "the citation should resolve: {diagnostics:#?}");

    // Rename the key in the editor only; the file on disk still has the old one.
    harness.change(&bib, &REFS.replace("knuth1984", "knuth1984tlp"));
    let events = harness.compile(&main);
    assert!(
        !diagnostics_in(&events, &main).is_empty(),
        "the citation should now be unresolved"
    );
}

/// The two publishers run on different clocks. A compile must not clear the
/// bibliography's squiggles on its way past.
#[test]
fn a_compile_leaves_bibliography_diagnostics_standing() {
    let mut harness = Harness::new();
    let bib = harness.open("refs.bib", "@misc{same, title={A}}\n@misc{same, title={B}}\n");
    let main = harness.open("main.typ", "= Title\n");
    assert!(!diagnostics_in(&harness.server().drain(), &bib).is_empty());

    let events = harness.compile(&main);
    let published: Vec<&serde_json::Value> = events
        .iter()
        .filter(|event| event.method == "textDocument/publishDiagnostics")
        .filter(|event| event.params["uri"] == json!(bib.as_str()))
        .map(|event| &event.params)
        .collect();

    assert!(
        published.is_empty(),
        "the compile republished into the `.bib` file: {published:#?}"
    );
}

#[test]
fn bibliography_symbols_are_its_entries_with_their_fields() {
    let mut harness = Harness::new();
    let uri = harness.open("refs.bib", REFS);

    let result = harness.request(
        "textDocument/documentSymbol",
        json!({ "textDocument": { "uri": uri.as_str() } }),
    );

    let entries = result.as_array().unwrap();
    assert_eq!(entries.len(), 3, "{entries:#?}");
    assert_eq!(entries[0]["name"], json!("knuth1984"));
    assert!(
        entries[0]["detail"].as_str().unwrap().contains("Literate Programming"),
        "{:#?}",
        entries[0]
    );

    let fields = entries[0]["children"].as_array().unwrap();
    assert!(fields.iter().any(|field| field["name"] == json!("author")));
}

#[test]
fn bibliography_folding_is_one_region_per_entry() {
    let mut harness = Harness::new();
    let uri = harness.open("refs.bib", REFS);

    let result = harness.request(
        "textDocument/foldingRange",
        json!({ "textDocument": { "uri": uri.as_str() } }),
    );
    assert_eq!(result.as_array().unwrap().len(), 3, "{result}");
}

#[test]
fn bibliography_hover_reads_the_entry_under_the_cursor() {
    let mut harness = Harness::new();
    let (uri, position) = harness.open_with_cursor(
        "refs.bib",
        "@article{knu/* CURSOR */th1984,\n  title = {Literate Programming},\n}\n",
    );

    let result = harness.request("textDocument/hover", at(&uri, position));
    let value = result["contents"]["value"].as_str().unwrap_or_default();
    assert!(value.contains("Literate Programming"), "got {value:?}");
    assert!(value.contains("article"), "got {value:?}");
}

#[test]
fn bibliography_hover_on_a_field_says_what_it_means() {
    let mut harness = Harness::new();
    let (uri, position) = harness
        .open_with_cursor("refs.bib", "@article{k,\n  jour/* CURSOR */nal = {TCJ},\n}\n");

    let result = harness.request("textDocument/hover", at(&uri, position));
    let value = result["contents"]["value"].as_str().unwrap_or_default();
    assert!(value.contains("journal"), "got {value:?}");
}

#[test]
fn bibliography_completion_offers_entry_types_then_field_names() {
    let mut harness = Harness::new();

    let (uri, position) = harness.open_with_cursor("refs.bib", "@/* CURSOR */\n");
    let result = harness.request("textDocument/completion", at(&uri, position));
    let items = result["items"].as_array().unwrap();
    let article = items
        .iter()
        .find(|item| item["label"] == json!("@article"))
        .unwrap_or_else(|| panic!("expected `@article`: {items:#?}"));
    let inserted = article["textEdit"]["newText"].as_str().unwrap();
    assert!(inserted.starts_with("@article{"), "{inserted}");
    assert!(inserted.contains("journal = {$4}"), "the skeleton needs tab stops: {inserted}");
    // The edit has to swallow the `@` the reader already typed.
    assert_eq!(article["textEdit"]["range"]["start"]["character"], json!(0));

    let (uri, position) =
        harness.open_with_cursor("refs.bib", "@article{k,\n  au/* CURSOR */\n}\n");
    let result = harness.request("textDocument/completion", at(&uri, position));
    let items = result["items"].as_array().unwrap();
    let author = items
        .iter()
        .find(|item| item["label"] == json!("author"))
        .unwrap_or_else(|| panic!("expected `author`: {items:#?}"));
    assert_eq!(author["textEdit"]["newText"], json!("author = {$1},"));
    assert!(
        author["sortText"].as_str().unwrap().starts_with('0'),
        "a required field sorts first"
    );
}

#[test]
fn a_crossref_completes_and_then_jumps_to_the_entry_it_names() {
    let mut harness = Harness::new();
    let text = "@book{whole,\n  title = {A Book},\n}\n\n\
                @inbook{part,\n  crossref = {who/* CURSOR */le},\n}\n";
    let (uri, position) = harness.open_with_cursor("refs.bib", text);

    let result = harness.request("textDocument/completion", at(&uri, position));
    let items = result["items"].as_array().unwrap();
    assert!(
        items.iter().any(|item| item["label"] == json!("whole")),
        "expected the other entry's key: {items:#?}"
    );

    let result = harness.request("textDocument/definition", at(&uri, position));
    assert_eq!(result["uri"], json!(uri.as_str()));
    assert_eq!(result["range"]["start"]["line"], json!(0), "{result}");
}

#[test]
fn bibliography_links_come_from_url_and_doi_fields() {
    let mut harness = Harness::new();
    let uri = harness.open("refs.bib", REFS);

    let result = harness.request(
        "textDocument/documentLink",
        json!({ "textDocument": { "uri": uri.as_str() } }),
    );
    let targets: Vec<&str> =
        result.as_array().unwrap().iter().map(|l| l["target"].as_str().unwrap()).collect();

    assert!(
        targets.contains(&"https://doi.org/10.1093/comjnl/27.2.97"),
        "a bare DOI needs the resolver in front of it: {targets:?}"
    );
    assert!(
        targets.iter().any(|target| target.contains("edwardtufte.com")),
        "{targets:?}"
    );
}

#[test]
fn bibliography_semantic_tokens_come_from_the_bibtex_parse() {
    let mut harness = Harness::new();
    let uri = harness.open("refs.bib", REFS);

    let result = harness.request(
        "textDocument/semanticTokens/full",
        json!({ "textDocument": { "uri": uri.as_str() } }),
    );
    let data = result["data"].as_array().unwrap();
    assert!(!data.is_empty());

    // The first token is `@article`: line 0, character 0, eight units long.
    assert_eq!(data[0], json!(0), "delta line");
    assert_eq!(data[1], json!(0), "delta start");
    assert_eq!(data[2], json!("@article".len()), "length");
}

#[test]
fn formatting_a_bibliography_normalises_it_rather_than_running_typstyle() {
    let mut harness = Harness::new();
    let uri = harness.open(
        "refs.bib",
        "@ARTICLE{ k ,Author={A},TITLE={T},journal={J},year={1984}}",
    );

    let result = harness.request(
        "textDocument/formatting",
        json!({
            "textDocument": { "uri": uri.as_str() },
            "options": { "tabSize": 2, "insertSpaces": true },
        }),
    );

    let edits = result.as_array().unwrap();
    assert_eq!(edits.len(), 1, "{edits:#?}");
    assert_eq!(
        edits[0]["newText"],
        json!(
            "@article{k,\n  author = {A},\n  title = {T},\n  \
             journal = {J},\n  year = {1984},\n}\n"
        )
    );
}

#[test]
fn formatting_leaves_a_broken_bibliography_alone() {
    let mut harness = Harness::new();
    let uri = harness.open("refs.bib", "@article{k, title = {unclosed\n");

    let result = harness.request(
        "textDocument/formatting",
        json!({
            "textDocument": { "uri": uri.as_str() },
            "options": { "tabSize": 2, "insertSpaces": true },
        }),
    );
    assert_eq!(result, json!(null), "a half-typed file must not be rewritten");
}

#[test]
fn a_citation_hovers_and_jumps_into_the_bibliography() {
    let mut harness = Harness::new();
    harness.open("refs.bib", REFS);
    let (main, position) = harness.open_with_cursor(
        "main.typ",
        "#bibliography(\"refs.bib\")\n\nSee @knu/* CURSOR */th1984.\n",
    );

    let hover = harness.request("textDocument/hover", at(&main, position));
    let value = hover["contents"]["value"].as_str().unwrap_or_default();
    assert!(
        value.contains("Literate Programming"),
        "the tooltip should read the bibliography: {value:?}"
    );

    let definition = harness.request("textDocument/definition", at(&main, position));
    assert!(
        definition["uri"].as_str().unwrap_or_default().ends_with("refs.bib"),
        "got {definition}"
    );
    assert_eq!(definition["range"]["start"]["line"], json!(0));
}

#[test]
fn citation_keys_are_offered_after_an_at_sign() {
    let mut harness = Harness::new();
    harness.open("refs.bib", REFS);
    let (main, position) =
        harness.open_with_cursor("main.typ", "See @/* CURSOR */\n");

    let result = harness.request("textDocument/completion", at(&main, position));
    let items = result["items"].as_array().unwrap();
    let citation = items
        .iter()
        .find(|item| item["label"] == json!("@madje2022"))
        .unwrap_or_else(|| panic!("expected the bibliography's keys: {items:#?}"));

    assert_eq!(citation["textEdit"]["newText"], json!("@madje2022"));
    assert_eq!(
        citation["textEdit"]["range"]["start"]["character"],
        json!(4),
        "the edit must replace the `@` the reader typed"
    );
    assert!(citation["detail"].as_str().unwrap().contains("Madje"), "{citation:#?}");
}

#[test]
fn renaming_a_citation_key_rewrites_the_entry_and_every_citation() {
    let mut harness = Harness::new();
    let main = harness.open("main.typ", "See @knuth1984 and @knuth1984 again.\n");
    let (bib, position) =
        harness.open_with_cursor("refs.bib", "@article{knu/* CURSOR */th1984,\n}\n");

    let prepared = harness.request("textDocument/prepareRename", at(&bib, position));
    assert_eq!(prepared["start"]["character"], json!("@article{".len()));

    let mut params = at(&bib, position);
    params["newName"] = json!("knuth1984tlp");
    let result = harness.request("textDocument/rename", params);

    let changes = result["changes"].as_object().unwrap();
    let in_bib = changes[bib.as_str()].as_array().unwrap();
    assert_eq!(in_bib.len(), 1, "the entry's key: {in_bib:#?}");
    assert_eq!(in_bib[0]["range"]["start"]["character"], json!("@article{".len()));

    let in_main = changes[main.as_str()].as_array().unwrap();
    assert_eq!(in_main.len(), 2, "both citations: {in_main:#?}");
    // The `@` stays; only the name is rewritten.
    assert_eq!(in_main[0]["range"]["start"]["character"], json!(5));
    assert_eq!(in_main[0]["newText"], json!("knuth1984tlp"));
}

#[test]
fn workspace_symbols_find_citation_keys() {
    let mut harness = Harness::new();
    let bib = harness.uri("refs.bib");
    harness
        .server()
        .on_notification("typst/workspaceFiles", json!({ "uris": [bib.as_str()] }));

    let result = harness.request("workspace/symbol", json!({ "query": "tufte" }));
    let symbols = result.as_array().unwrap();
    assert!(
        symbols.iter().any(|symbol| symbol["name"] == json!("tufte2001")),
        "{symbols:#?}"
    );
}

#[test]
fn typst_only_features_decline_in_a_bibliography() {
    let mut harness = Harness::new();
    let uri = harness.open("refs.bib", REFS);
    let document = json!({ "textDocument": { "uri": uri.as_str() } });

    assert_eq!(harness.request("textDocument/codeLens", document.clone()), json!(null));

    let mut params = document.clone();
    params["range"] = json!({
        "start": { "line": 0, "character": 0 },
        "end": { "line": 0, "character": 0 },
    });
    params["context"] = json!({ "diagnostics": [] });
    assert_eq!(harness.request("textDocument/codeAction", params), json!(null));
}

#[test]
fn html_export_produces_a_document() {
    let mut harness = Harness::new();
    let uri = harness.open("main.typ", "= Title\n\nSome body text.\n");
    harness.compile(&uri);

    let result = harness.request("typst/export", json!({ "format": "html" }));
    assert_eq!(result["extension"], json!("html"));

    let encoded = result["files"][0].as_str().unwrap();
    let decoded = decode_base64(encoded);
    assert!(decoded.contains("<html"), "expected an HTML document: {decoded:.120}");
    assert!(decoded.contains("Title"));
}

/// Minimal base64 decoder, so the test can read what the server produced.
fn decode_base64(text: &str) -> String {
    const ALPHABET: &[u8] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut bits = Vec::new();
    for byte in text.bytes().filter(|b| *b != b'=') {
        let value = ALPHABET.iter().position(|c| *c == byte).expect("base64 alphabet");
        for shift in (0..6).rev() {
            bits.push((value >> shift) & 1 == 1);
        }
    }

    let bytes: Vec<u8> = bits
        .as_chunks::<8>()
        .0
        .iter()
        .map(|chunk| chunk.iter().fold(0u8, |acc, bit| (acc << 1) | u8::from(*bit)))
        .collect();

    String::from_utf8_lossy(&bytes).into_owned()
}
