//! P2-17: per-feature tests over fixtures with a cursor marker.
//!
//! Everything here goes through `on_request` / `on_notification`, so the tests
//! exercise the same dispatch path the WASM binding does — a feature that works
//! here works in the editor, modulo transport.

mod support;

use serde_json::json;
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
