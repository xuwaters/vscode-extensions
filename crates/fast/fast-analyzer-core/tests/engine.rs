//! Engine-level tests: whole diagnostic passes and position queries over
//! constructed `upsertFile` payloads — the JSON the plugin would send, with
//! no compiler anywhere near the test (design/crates.md, Testing).

use fast_analyzer_core::protocol::*;
use fast_analyzer_core::Engine;

/// Turn a template written with real `${…}` into substituted text plus
/// placeholder facts. Every expression defaults to a reactive arrow; tests
/// override individual entries to model other shapes.
fn substitute(source: &str) -> (String, Vec<PlaceholderFact>) {
    let bytes = source.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut placeholders = Vec::new();
    let mut pos = 0;
    let mut index = 0u32;
    while pos < bytes.len() {
        if bytes[pos] == b'$' && pos + 1 < bytes.len() && bytes[pos + 1] == b'{' {
            let start = pos;
            let mut depth = 0usize;
            while pos < bytes.len() {
                match bytes[pos] {
                    b'{' => depth += 1,
                    b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            pos += 1;
                            break;
                        }
                    }
                    _ => {}
                }
                pos += 1;
            }
            placeholders.push(PlaceholderFact {
                index,
                start: start as u32,
                end: pos as u32,
                expr: Some(arrow()),
            });
            index += 1;
            out.resize(pos, b'_');
        } else {
            out.push(bytes[pos]);
            pos += 1;
        }
    }
    (String::from_utf8(out).unwrap(), placeholders)
}

fn arrow() -> ExprInfo {
    ExprInfo {
        kind: "arrow".into(),
        is_function_type: Some(true),
        is_constant: Some(false),
        is_directive_value: false,
        directive: None,
        is_partial: false,
    }
}

fn value_read() -> ExprInfo {
    ExprInfo {
        kind: "propertyAccess".into(),
        is_function_type: Some(false),
        is_constant: Some(false),
        is_directive_value: false,
        directive: None,
        is_partial: false,
    }
}

fn directive(name: &str, arg: Option<(&str, u32, u32)>) -> ExprInfo {
    ExprInfo {
        kind: "call".into(),
        is_function_type: Some(false),
        is_constant: Some(false),
        is_directive_value: true,
        directive: Some(DirectiveInfo {
            name: name.into(),
            arg_string: arg.map(|(s, _, _)| s.to_string()),
            arg_start: arg.map(|(_, s, _)| s),
            arg_end: arg.map(|(_, _, e)| e),
        }),
        is_partial: false,
    }
}

fn doc(id: &str, file: &str, source: &str) -> VirtualDocumentFact {
    let (text, placeholders) = substitute(source);
    VirtualDocumentFact {
        id: id.into(),
        file_name: file.into(),
        template_start: 100,
        kind: "html".into(),
        text,
        placeholders,
        source_type_id: Some(1),
        parent_type_id: None,
        source_type_name: Some("CsvGrid".into()),
        source_members: Some(vec![
            member_fact("findInput", "HTMLInputElement"),
            member_fact("hasHeader", "boolean"),
            member_fact("tableEl", "HTMLDivElement"),
        ]),
        component_tag: None,
        type_arg_insert_offset: None,
    }
}

fn member_fact(name: &str, type_text: &str) -> SourceMember {
    SourceMember {
        name: name.into(),
        type_text: Some(type_text.into()),
        documentation: None,
        decl_span: Some(FileSpan {
            file_name: "/proj/element.ts".into(),
            start: 500,
            end: 510,
        }),
        is_function: false,
    }
}

fn csv_grid_component() -> ComponentFact {
    ComponentFact {
        tag_name: Some("csv-grid".into()),
        class_name: "CsvGrid".into(),
        tag_name_span: Some(FileSpan {
            file_name: "/proj/element.ts".into(),
            start: 30,
            end: 38,
        }),
        decl_span: Some(FileSpan {
            file_name: "/proj/element.ts".into(),
            start: 60,
            end: 67,
        }),
        declaration_id: Some(10),
        source_type_id: Some(1),
        attributes: vec![MemberFact {
            name: "row-height".into(),
            property_name: Some("rowHeight".into()),
            mode: Some("reflect".into()),
            type_text: Some("number".into()),
            declaration_id: Some(11),
            decl_span: Some(FileSpan {
                file_name: "/proj/element.ts".into(),
                start: 120,
                end: 129,
            }),
            origin: "decorator".into(),
            ..MemberFact::default()
        }],
        properties: vec![MemberFact {
            name: "hasHeader".into(),
            type_text: Some("boolean".into()),
            declaration_id: Some(12),
            decl_span: Some(FileSpan {
                file_name: "/proj/element.ts".into(),
                start: 140,
                end: 149,
            }),
            origin: "decorator".into(),
            ..MemberFact::default()
        }],
        events: vec![EventFact {
            name: "selectionchanged".into(),
            type_text: Some("Selection".into()),
            decl_span: Some(FileSpan {
                file_name: "/proj/element.ts".into(),
                start: 160,
                end: 176,
            }),
            documentation: None,
        }],
        slots: vec![NamedFact {
            name: "toolbar".into(),
            documentation: None,
            decl_span: Some(FileSpan {
                file_name: "/proj/element.ts".into(),
                start: 180,
                end: 187,
            }),
        }],
        has_shadow_root: true,
        origin: "decorator".into(),
        ..ComponentFact::default()
    }
}

fn strict_engine() -> Engine {
    let mut engine = Engine::new();
    engine.set_config(Config {
        strict: true,
        ..Config::default()
    });
    engine
}

fn upsert(engine: &mut Engine, file: &str, components: Vec<ComponentFact>, docs: Vec<VirtualDocumentFact>) {
    upsert_with_deps(engine, file, components, docs, vec![]);
}

fn upsert_with_deps(
    engine: &mut Engine,
    file: &str,
    components: Vec<ComponentFact>,
    docs: Vec<VirtualDocumentFact>,
    dependencies: Vec<String>,
) {
    engine.upsert_file(UpsertFile {
        file_name: file.into(),
        dependencies,
        node_module_dependencies: vec![],
        components,
        documents: docs,
        global_events: vec![],
    });
}

/// The synthetic ambient file, carrying an `HTMLElementEventMap` augmentation.
fn upsert_global_events(engine: &mut Engine, events: Vec<EventFact>) {
    engine.upsert_file(UpsertFile {
        file_name: "fast-element-ultra:tag-name-map".into(),
        dependencies: vec![],
        node_module_dependencies: vec![],
        components: vec![],
        documents: vec![],
        global_events: events,
    });
}

fn rule_ids(result: &AnalyzeResult) -> Vec<&str> {
    result.diagnostics.iter().map(|d| d.rule_id.as_str()).collect()
}

// ------------------------------------------------------------------- rules

#[test]
fn clean_template_is_silent_in_strict_mode() {
    let mut engine = strict_engine();
    let template = doc(
        "t1",
        "/proj/template.ts",
        r#"<div class="chrome" role="toolbar" aria-label="Table tools">
            <button class="btn ${(x) => (x.hasHeader ? 'on' : '')}"
                aria-pressed="${(x) => String(x.hasHeader)}"
                @click="${(x) => x.toggleHeader()}">
              <svg class="icon" viewBox="0 0 16 16" aria-hidden="true" focusable="false">
                <path d="M2 3h12v3H2z" fill="currentColor" opacity="0.8" />
              </svg>
            </button>
            <input :value="${(x) => x.query}" @keydown="${(x, c) => true}" />
          </div>"#,
    );
    upsert(&mut engine, "/proj/template.ts", vec![], vec![template]);
    let result = engine.analyze("t1").unwrap();
    assert_eq!(rule_ids(&result), Vec::<&str>::new(), "{:#?}", result.diagnostics);
    // But the type questions were declared: class (mixed literal is not a
    // single-placeholder binding, so: aria-pressed, :value) and the events.
    assert!(result.facts.iter().any(|f| f.kind == "attribute" && f.member_name.as_deref() == Some("aria-pressed")));
    assert!(result.facts.iter().any(|f| f.kind == "property" && f.member_name.as_deref() == Some("value")));
    assert!(result.facts.iter().any(|f| f.kind == "event" && f.member_name.as_deref() == Some("click")));
}

#[test]
fn unknown_tag_reported_with_suggestion() {
    let mut engine = strict_engine();
    upsert(
        &mut engine,
        "/proj/t.ts",
        vec![csv_grid_component()],
        vec![doc("t1", "/proj/t.ts", "<csv-gird></csv-gird>")],
    );
    let result = engine.analyze("t1").unwrap();
    assert_eq!(rule_ids(&result), vec!["no-unknown-tag-name"]);
    assert!(result.diagnostics[0].message.contains("csv-grid"), "{}", result.diagnostics[0].message);
    assert!(!result.diagnostics[0].fixes.is_empty());
}

#[test]
fn known_component_is_not_unknown() {
    let mut engine = strict_engine();
    upsert(
        &mut engine,
        "/proj/element.ts",
        vec![csv_grid_component()],
        vec![],
    );
    upsert_with_deps(
        &mut engine,
        "/proj/t.ts",
        vec![],
        vec![doc("t1", "/proj/t.ts", "<csv-grid></csv-grid>")],
        vec!["/proj/element.ts".into()],
    );
    let result = engine.analyze("t1").unwrap();
    assert_eq!(rule_ids(&result), Vec::<&str>::new(), "{:#?}", result.diagnostics);
}

#[test]
fn missing_import_reported_when_unreachable() {
    let mut engine = strict_engine();
    upsert(&mut engine, "/proj/element.ts", vec![csv_grid_component()], vec![]);
    upsert(
        &mut engine,
        "/proj/t.ts",
        vec![],
        vec![doc("t1", "/proj/t.ts", "<csv-grid></csv-grid>")],
    );
    let result = engine.analyze("t1").unwrap();
    assert_eq!(rule_ids(&result), vec!["no-missing-import"]);
    assert_eq!(
        result.diagnostics[0].fixes[0].command.as_ref().unwrap().kind,
        "addImport"
    );
}

#[test]
fn transitive_import_is_reachable() {
    let mut engine = strict_engine();
    upsert(&mut engine, "/proj/element.ts", vec![csv_grid_component()], vec![]);
    upsert_with_deps(&mut engine, "/proj/middle.ts", vec![], vec![], vec!["/proj/element.ts".into()]);
    upsert_with_deps(
        &mut engine,
        "/proj/t.ts",
        vec![],
        vec![doc("t1", "/proj/t.ts", "<csv-grid></csv-grid>")],
        vec!["/proj/middle.ts".into()],
    );
    let result = engine.analyze("t1").unwrap();
    assert_eq!(rule_ids(&result), Vec::<&str>::new());
}

#[test]
fn unclosed_tag_reported_with_insertion_fix() {
    let mut engine = strict_engine();
    upsert(
        &mut engine,
        "/proj/t.ts",
        vec![],
        vec![doc("t1", "/proj/t.ts", "<div><span>text</div>")],
    );
    let result = engine.analyze("t1").unwrap();
    assert_eq!(rule_ids(&result), vec!["no-unclosed-tag"]);
    let fix = &result.diagnostics[0].fixes[0];
    assert_eq!(fix.edits[0].new_text, "</span>");
}

#[test]
fn self_closed_custom_element_reported() {
    let mut engine = strict_engine();
    upsert(
        &mut engine,
        "/proj/element.ts",
        vec![csv_grid_component()],
        vec![doc("t1", "/proj/element.ts", "<csv-grid/>")],
    );
    let result = engine.analyze("t1").unwrap();
    assert_eq!(rule_ids(&result), vec!["no-unclosed-tag"]);
    assert!(result.diagnostics[0].message.contains("self-closed"));
}

#[test]
fn svg_self_closing_is_fine() {
    let mut engine = strict_engine();
    upsert(
        &mut engine,
        "/proj/t.ts",
        vec![],
        vec![doc(
            "t1",
            "/proj/t.ts",
            r#"<svg viewBox="0 0 16 16"><circle cx="7" cy="7" r="4.2" fill="none" stroke="currentColor" stroke-width="1.4" /></svg>"#,
        )],
    );
    let result = engine.analyze("t1").unwrap();
    assert_eq!(rule_ids(&result), Vec::<&str>::new(), "{:#?}", result.diagnostics);
}

#[test]
fn unknown_attribute_with_suggestion() {
    let mut engine = strict_engine();
    upsert(
        &mut engine,
        "/proj/t.ts",
        vec![],
        vec![doc("t1", "/proj/t.ts", r#"<button aria-pressd="true">x</button>"#)],
    );
    let result = engine.analyze("t1").unwrap();
    assert_eq!(rule_ids(&result), vec!["no-unknown-attribute"]);
    assert!(result.diagnostics[0].message.contains("aria-pressed"));
}

#[test]
fn data_attributes_are_always_fine() {
    let mut engine = strict_engine();
    upsert(
        &mut engine,
        "/proj/t.ts",
        vec![],
        vec![doc("t1", "/proj/t.ts", r#"<div data-whatever="1" part="frame">x</div>"#)],
    );
    let result = engine.analyze("t1").unwrap();
    assert_eq!(rule_ids(&result), Vec::<&str>::new(), "{:#?}", result.diagnostics);
}

#[test]
fn component_attribute_known_and_unknown_property() {
    let mut engine = strict_engine();
    upsert(&mut engine, "/proj/element.ts", vec![csv_grid_component()], vec![]);
    upsert_with_deps(
        &mut engine,
        "/proj/t.ts",
        vec![],
        vec![doc(
            "t1",
            "/proj/t.ts",
            r#"<csv-grid row-height="24" :hasHedaer="${(x) => x.hasHeader}"></csv-grid>"#,
        )],
        vec!["/proj/element.ts".into()],
    );
    let result = engine.analyze("t1").unwrap();
    assert_eq!(rule_ids(&result), vec!["no-unknown-property"]);
    assert!(result.diagnostics[0].message.contains("hasHeader"));
}

#[test]
fn unknown_event_on_builtin_and_known_dom_event() {
    let mut engine = strict_engine();
    upsert(
        &mut engine,
        "/proj/t.ts",
        vec![],
        vec![doc(
            "t1",
            "/proj/t.ts",
            r#"<button @clik="${(x) => x.go()}" @pointerdown="${(x) => x.down()}">x</button>"#,
        )],
    );
    let result = engine.analyze("t1").unwrap();
    assert_eq!(rule_ids(&result), vec!["no-unknown-event"]);
    assert!(result.diagnostics[0].message.contains("click"));
}

#[test]
fn component_event_is_known() {
    let mut engine = strict_engine();
    upsert(&mut engine, "/proj/element.ts", vec![csv_grid_component()], vec![]);
    upsert_with_deps(
        &mut engine,
        "/proj/t.ts",
        vec![],
        vec![doc(
            "t1",
            "/proj/t.ts",
            r#"<csv-grid @selectionchanged="${(x) => x.onSel()}"></csv-grid>"#,
        )],
        vec!["/proj/element.ts".into()],
    );
    let result = engine.analyze("t1").unwrap();
    assert_eq!(rule_ids(&result), Vec::<&str>::new(), "{:#?}", result.diagnostics);
}

#[test]
fn html_element_event_map_augmentation_is_global() {
    let mut engine = strict_engine();
    upsert_global_events(
        &mut engine,
        vec![EventFact {
            name: "tab-select".into(),
            type_text: Some("{ id: number; }".into()),
            decl_span: Some(FileSpan {
                file_name: "/proj/events.ts".into(),
                start: 42,
                end: 52,
            }),
            documentation: Some("A tab was chosen.".into()),
        }],
    );
    let source = r#"<div @tab-select="${(x) => x.pick()}" @tab-selct="${(x) => x.pick()}"></div>"#;
    upsert(&mut engine, "/proj/t.ts", vec![], vec![doc("t1", "/proj/t.ts", source)]);

    // Known everywhere — the augmentation names no tag — and the near miss
    // beside it still suggests the real one.
    let result = engine.analyze("t1").unwrap();
    assert_eq!(rule_ids(&result), vec!["no-unknown-event"]);
    assert!(result.diagnostics[0].message.contains("tab-select"), "{:#?}", result.diagnostics);

    let offset = source.find("@tab-select").unwrap() + 2;
    let hover = engine.query(serde_json::from_value(serde_json::json!({
        "type": "quickInfo", "documentId": "t1", "offset": offset
    })).unwrap());
    let contents = hover["contents"].as_str().unwrap();
    assert!(contents.contains("CustomEvent<{ id: number; }>"), "{contents}");
    assert!(contents.contains("A tab was chosen."), "{contents}");

    let definition = engine.query(serde_json::from_value(serde_json::json!({
        "type": "definition", "documentId": "t1", "offset": offset
    })).unwrap());
    assert_eq!(definition["targets"][0]["fileName"], "/proj/events.ts");
    assert_eq!(definition["targets"][0]["start"], 42);

    let completions = engine.query(serde_json::from_value(serde_json::json!({
        "type": "completions", "documentId": "t1", "offset": 5
    })).unwrap());
    let names: Vec<String> = completions["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["name"].as_str().unwrap().to_string())
        .collect();
    // Offset 5 sits on the `@` already typed, so the names come unprefixed.
    assert!(names.contains(&"tab-select".to_string()), "{names:?}");
    assert_eq!(names.iter().filter(|n| *n == "click").count(), 1, "{names:?}");
}

#[test]
fn removing_the_ambient_file_takes_its_global_events_with_it() {
    let mut engine = strict_engine();
    upsert_global_events(
        &mut engine,
        vec![EventFact {
            name: "tab-select".into(),
            ..EventFact::default()
        }],
    );
    upsert(
        &mut engine,
        "/proj/t.ts",
        vec![],
        vec![doc("t1", "/proj/t.ts", r#"<div @tab-select="${(x) => x.pick()}"></div>"#)],
    );
    assert_eq!(rule_ids(&engine.analyze("t1").unwrap()), Vec::<&str>::new());

    // The program changed and the augmentation went away.
    upsert_global_events(&mut engine, vec![]);
    assert_eq!(rule_ids(&engine.analyze("t1").unwrap()), vec!["no-unknown-event"]);
}

#[test]
fn expressionless_property_binding() {
    let mut engine = strict_engine();
    upsert(&mut engine, "/proj/element.ts", vec![csv_grid_component()], vec![]);
    upsert_with_deps(
        &mut engine,
        "/proj/t.ts",
        vec![],
        vec![doc("t1", "/proj/t.ts", r#"<csv-grid :hasHeader="yes"></csv-grid>"#)],
        vec!["/proj/element.ts".into()],
    );
    let result = engine.analyze("t1").unwrap();
    assert_eq!(rule_ids(&result), vec!["no-expressionless-property-binding"]);
    // The fix drops the ':'.
    assert_eq!(result.diagnostics[0].fixes[0].edits[0].new_text, "");
}

#[test]
fn mixed_binding_catches_stray_slash() {
    let mut engine = strict_engine();
    upsert(
        &mut engine,
        "/proj/t.ts",
        vec![],
        vec![doc("t1", "/proj/t.ts", r#"<input value=${(x) => x.v}/ >"#)],
    );
    let result = engine.analyze("t1").unwrap();
    assert!(rule_ids(&result).contains(&"no-unintended-mixed-binding"));
}

#[test]
fn non_reactive_binding_fires_on_value_reads_only() {
    let mut engine = strict_engine();
    let mut template = doc(
        "t1",
        "/proj/t.ts",
        r#"<span>${grid.count}</span><span>${shortcut('a', 'b')}</span><span>${SOME_CONST}</span>"#,
    );
    // ${grid.count}: a property access on a mutable object.
    template.placeholders[0].expr = Some(value_read());
    // ${shortcut(...)}: a call — plausibly a deliberate one-time value.
    template.placeholders[1].expr = Some(ExprInfo {
        kind: "call".into(),
        is_function_type: Some(false),
        is_constant: Some(false),
        ..arrow()
    });
    // ${SOME_CONST}: a constant.
    template.placeholders[2].expr = Some(ExprInfo {
        kind: "identifier".into(),
        is_function_type: Some(false),
        is_constant: Some(true),
        ..arrow()
    });
    upsert(&mut engine, "/proj/t.ts", vec![], vec![template]);
    let result = engine.analyze("t1").unwrap();
    assert_eq!(rule_ids(&result), vec!["no-non-reactive-binding"]);
    let d = &result.diagnostics[0];
    // The fix wraps in an arrow, inserting right after `${`.
    assert_eq!(d.fixes[0].edits[0].new_text, "() => ");
    assert_eq!(d.fixes[0].edits[0].start, d.start + 2);
}

#[test]
fn template_value_in_content_is_fine() {
    let mut engine = strict_engine();
    let mut template = doc("t1", "/proj/t.ts", "<div>${chrome}</div>");
    template.placeholders[0].expr = Some(ExprInfo {
        kind: "identifier".into(),
        is_function_type: Some(false),
        is_constant: Some(false),
        is_directive_value: true,
        ..arrow()
    });
    upsert(&mut engine, "/proj/t.ts", vec![], vec![template]);
    let result = engine.analyze("t1").unwrap();
    assert_eq!(rule_ids(&result), Vec::<&str>::new());
}

#[test]
fn directive_positions() {
    let mut engine = strict_engine();
    // when(...) in an attribute value: wrong.
    let mut t1 = doc("t1", "/proj/t.ts", r#"<div class="${when((x) => x.a, b)}">x</div>"#);
    t1.placeholders[0].expr = Some(directive("when", None));
    // ref(...) in content: wrong.
    let mut t2 = doc("t2", "/proj/t.ts", "<div>${ref('tableEl')}</div>");
    t2.placeholders[0].expr = Some(directive("ref", Some(("tableEl", 10, 17))));
    // ref between attributes: right.
    let mut t3 = doc("t3", "/proj/t.ts", "<div ${ref('tableEl')}>x</div>");
    t3.placeholders[0].expr = Some(directive("ref", Some(("tableEl", 11, 18))));
    upsert(&mut engine, "/proj/t.ts", vec![], vec![t1, t2, t3]);
    assert_eq!(rule_ids(&engine.analyze("t1").unwrap()), vec!["no-invalid-directive-binding"]);
    assert_eq!(rule_ids(&engine.analyze("t2").unwrap()), vec!["no-invalid-directive-binding"]);
    assert_eq!(rule_ids(&engine.analyze("t3").unwrap()), Vec::<&str>::new());
}

#[test]
fn directive_target_checked_against_source_members() {
    let mut engine = strict_engine();
    let source = "<input ${ref('findInpt')} />";
    let arg = source.find("findInpt").unwrap() as u32;
    let mut template = doc("t1", "/proj/t.ts", source);
    template.placeholders[0].expr =
        Some(directive("ref", Some(("findInpt", arg, arg + 8))));
    upsert(&mut engine, "/proj/t.ts", vec![], vec![template]);
    let result = engine.analyze("t1").unwrap();
    assert_eq!(rule_ids(&result), vec!["no-invalid-directive-target"]);
    assert!(result.diagnostics[0].message.contains("findInput"));
    assert_eq!(result.diagnostics[0].fixes[0].edits[0].new_text, "findInput");
    assert_eq!(result.diagnostics[0].start, arg);
}

#[test]
fn slot_on_light_dom_component() {
    let mut engine = strict_engine();
    let mut light = csv_grid_component();
    light.tag_name = Some("typst-preview".into());
    light.class_name = "TypstPreview".into();
    light.has_shadow_root = false;
    let mut template = doc("t1", "/proj/element.ts", "<div><slot></slot></div>");
    template.component_tag = Some("typst-preview".into());
    upsert(&mut engine, "/proj/element.ts", vec![light], vec![template]);
    let result = engine.analyze("t1").unwrap();
    assert_eq!(rule_ids(&result), vec!["no-slot-without-shadow-root"]);
}

#[test]
fn unknown_slot_name() {
    let mut engine = strict_engine();
    upsert(&mut engine, "/proj/element.ts", vec![csv_grid_component()], vec![]);
    upsert_with_deps(
        &mut engine,
        "/proj/t.ts",
        vec![],
        vec![doc(
            "t1",
            "/proj/t.ts",
            r#"<csv-grid><div slot="toolbr">x</div></csv-grid>"#,
        )],
        vec!["/proj/element.ts".into()],
    );
    let result = engine.analyze("t1").unwrap();
    assert_eq!(rule_ids(&result), vec!["no-unknown-slot"]);
    assert!(result.diagnostics[0].message.contains("toolbar"));
}

#[test]
fn untyped_component_template() {
    let mut engine = strict_engine();
    let mut component = csv_grid_component();
    component.template_document_id = Some("t1".into());
    let mut template = doc("t1", "/proj/element.ts", "<div>x</div>");
    template.source_type_id = None;
    template.source_type_name = None;
    template.source_members = None;
    template.component_tag = Some("csv-grid".into());
    template.type_arg_insert_offset = Some(95);
    upsert(&mut engine, "/proj/element.ts", vec![component], vec![template]);
    let result = engine.analyze("t1").unwrap();
    assert_eq!(rule_ids(&result), vec!["no-untyped-template"]);
    let fix = &result.diagnostics[0].fixes[0];
    assert_eq!(fix.edits[0].new_text, "<CsvGrid>");
    assert_eq!(fix.edits[0].file_name.as_deref(), Some("/proj/element.ts"));
}

#[test]
fn partial_template_is_not_analyzed() {
    let mut engine = strict_engine();
    let mut template = doc("t1", "/proj/t.ts", "<butto>${html.partial(raw)}");
    template.placeholders[0].expr = Some(ExprInfo {
        is_partial: true,
        ..arrow()
    });
    upsert(&mut engine, "/proj/t.ts", vec![], vec![template]);
    let result = engine.analyze("t1").unwrap();
    // The unclosed <butto> is NOT reported: analysis stopped.
    assert_eq!(rule_ids(&result), vec!["template-not-analyzed"]);
    assert_eq!(result.diagnostics[0].severity, Severity::Suggestion);
}

#[test]
fn literal_event_binding_is_noncallable() {
    let mut engine = strict_engine();
    upsert(
        &mut engine,
        "/proj/t.ts",
        vec![],
        vec![doc("t1", "/proj/t.ts", r#"<button @click="handler">x</button>"#)],
    );
    let result = engine.analyze("t1").unwrap();
    assert_eq!(rule_ids(&result), vec!["no-noncallable-event-binding"]);
}

#[test]
fn boolean_binding_emits_fact_with_target() {
    let mut engine = strict_engine();
    upsert(&mut engine, "/proj/element.ts", vec![csv_grid_component()], vec![]);
    upsert_with_deps(
        &mut engine,
        "/proj/t.ts",
        vec![],
        vec![doc(
            "t1",
            "/proj/t.ts",
            r#"<csv-grid ?row-height="${(x) => x.h}"></csv-grid>"#,
        )],
        vec!["/proj/element.ts".into()],
    );
    let result = engine.analyze("t1").unwrap();
    let fact = result
        .facts
        .iter()
        .find(|f| f.kind == "booleanAttribute")
        .expect("boolean fact");
    assert_eq!(fact.target_declaration_id, Some(11));
    assert_eq!(fact.tag_name, "csv-grid");
}

#[test]
fn duplicate_and_invalid_tag_names() {
    let mut engine = strict_engine();
    let mut a = csv_grid_component();
    a.tag_name = Some("grid".into()); // no hyphen: invalid
    let mut b = csv_grid_component();
    b.class_name = "OtherGrid".into();
    b.decl_span = Some(FileSpan {
        file_name: "/proj/element.ts".into(),
        start: 1,
        end: 9,
    });
    // Two components registering csv-grid, plus the invalid "grid".
    upsert(
        &mut engine,
        "/proj/element.ts",
        vec![a, csv_grid_component(), b],
        vec![],
    );
    let diags = engine.file_diagnostics("/proj/element.ts");
    let ids: Vec<&str> = diags.iter().map(|d| d.rule_id.as_str()).collect();
    assert!(ids.contains(&"no-invalid-tag-name"), "{ids:?}");
    assert!(ids.contains(&"no-duplicate-tag-name"), "{ids:?}");
}

#[test]
fn severity_gate_silences_rules() {
    let mut engine = Engine::new();
    engine.set_config(Config::default()); // not strict
    upsert(
        &mut engine,
        "/proj/t.ts",
        vec![],
        vec![doc("t1", "/proj/t.ts", "<zzz-unknown></zzz-unknown>")],
    );
    // no-unknown-tag-name defaults to off outside strict mode.
    let result = engine.analyze("t1").unwrap();
    assert_eq!(rule_ids(&result), Vec::<&str>::new());
}

// -------------------------------------------------------------------- ide

#[test]
fn tag_completions_offer_components_first() {
    let mut engine = strict_engine();
    upsert(&mut engine, "/proj/element.ts", vec![csv_grid_component()], vec![]);
    upsert_with_deps(
        &mut engine,
        "/proj/t.ts",
        vec![],
        vec![doc("t1", "/proj/t.ts", "<div><csv</div>")],
        vec!["/proj/element.ts".into()],
    );
    let value = engine.query(serde_json::from_value(serde_json::json!({
        "type": "completions", "documentId": "t1", "offset": 9
    })).unwrap());
    let items = value["items"].as_array().unwrap();
    let csv = items.iter().find(|i| i["name"] == "csv-grid").expect("csv-grid offered");
    assert_eq!(csv["sortText"], "0");
    assert!(items.iter().any(|i| i["name"] == "div"));
}

#[test]
fn attribute_completions_cover_all_binding_forms() {
    let mut engine = strict_engine();
    upsert(&mut engine, "/proj/element.ts", vec![csv_grid_component()], vec![]);
    let source = "<csv-grid ></csv-grid>";
    upsert_with_deps(
        &mut engine,
        "/proj/t.ts",
        vec![],
        vec![doc("t1", "/proj/t.ts", source)],
        vec!["/proj/element.ts".into()],
    );
    let value = engine.query(serde_json::from_value(serde_json::json!({
        "type": "completions", "documentId": "t1", "offset": 10
    })).unwrap());
    let names: Vec<String> = value["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["name"].as_str().unwrap().to_string())
        .collect();
    assert!(names.contains(&"row-height".to_string()), "{names:?}");
    assert!(names.contains(&":hasHeader".to_string()), "{names:?}");
    assert!(names.contains(&"@selectionchanged".to_string()), "{names:?}");
    assert!(names.contains(&"@click".to_string()));
    assert!(names.contains(&"class".to_string()));
}

#[test]
fn quick_info_on_component_tag() {
    let mut engine = strict_engine();
    upsert(
        &mut engine,
        "/proj/element.ts",
        vec![csv_grid_component()],
        vec![doc("t1", "/proj/element.ts", "<csv-grid></csv-grid>")],
    );
    let value = engine.query(serde_json::from_value(serde_json::json!({
        "type": "quickInfo", "documentId": "t1", "offset": 3
    })).unwrap());
    let contents = value["contents"].as_str().unwrap();
    assert!(contents.contains("CsvGrid"));
    assert!(contents.contains("toolbar"), "slots listed: {contents}");
}

#[test]
fn definition_of_attribute_lands_on_member() {
    let mut engine = strict_engine();
    upsert(&mut engine, "/proj/element.ts", vec![csv_grid_component()], vec![]);
    let source = r#"<csv-grid row-height="24"></csv-grid>"#;
    upsert_with_deps(
        &mut engine,
        "/proj/t.ts",
        vec![],
        vec![doc("t1", "/proj/t.ts", source)],
        vec!["/proj/element.ts".into()],
    );
    let offset = source.find("row-height").unwrap() + 2;
    let value = engine.query(serde_json::from_value(serde_json::json!({
        "type": "definition", "documentId": "t1", "offset": offset
    })).unwrap());
    assert_eq!(value["targets"][0]["fileName"], "/proj/element.ts");
    assert_eq!(value["targets"][0]["start"], 120);
}

#[test]
fn member_rename_locations_cover_bindings_and_ref_strings() {
    let mut engine = strict_engine();
    upsert(&mut engine, "/proj/element.ts", vec![csv_grid_component()], vec![]);
    let source = r#"<csv-grid :hasHeader="${(x) => x.h}"></csv-grid><input ${ref('hasHeader')} />"#;
    let mut template = doc("t1", "/proj/t.ts", source);
    let arg = source.find("'hasHeader'").unwrap() as u32 + 1;
    template.placeholders[1].expr = Some(directive("ref", Some(("hasHeader", arg, arg + 9))));
    upsert_with_deps(
        &mut engine,
        "/proj/t.ts",
        vec![],
        vec![template],
        vec!["/proj/element.ts".into()],
    );
    let value = engine.query(serde_json::from_value(serde_json::json!({
        "type": "memberRenameLocations", "tag": "csv-grid", "sourceTypeId": 1, "name": "hasHeader"
    })).unwrap());
    let spans = value.as_array().unwrap();
    // One for the :hasHeader attribute name, one for the ref('…') string.
    assert_eq!(spans.len(), 2, "{spans:?}");
    // Every span is absolute: template_start (100) added.
    for span in spans {
        assert!(span["start"].as_u64().unwrap() >= 100);
    }
}

#[test]
fn tag_rename_includes_declaration_string() {
    let mut engine = strict_engine();
    upsert(
        &mut engine,
        "/proj/element.ts",
        vec![csv_grid_component()],
        vec![doc("t1", "/proj/element.ts", "<csv-grid></csv-grid>")],
    );
    let value = engine.query(serde_json::from_value(serde_json::json!({
        "type": "tagRenameLocations", "tag": "csv-grid"
    })).unwrap());
    let spans = value.as_array().unwrap();
    // Open tag, close tag, and the string literal in the registration.
    assert_eq!(spans.len(), 3, "{spans:?}");
}

#[test]
fn closing_tag_completion() {
    let mut engine = strict_engine();
    let source = "<div><button>";
    upsert(
        &mut engine,
        "/proj/t.ts",
        vec![],
        vec![doc("t1", "/proj/t.ts", source)],
    );
    let value = engine.query(serde_json::from_value(serde_json::json!({
        "type": "closingTag", "documentId": "t1", "offset": source.len()
    })).unwrap());
    assert_eq!(value["newText"], "</button>");
}

#[test]
fn folding_ranges_for_multiline_elements() {
    let mut engine = strict_engine();
    upsert(
        &mut engine,
        "/proj/t.ts",
        vec![],
        vec![doc("t1", "/proj/t.ts", "<div>\n  <span>x</span>\n</div>")],
    );
    let value = engine.query(serde_json::from_value(serde_json::json!({
        "type": "folding", "documentId": "t1"
    })).unwrap());
    assert_eq!(value.as_array().unwrap().len(), 1);
}

#[test]
fn code_fixes_at_range() {
    let mut engine = strict_engine();
    upsert(
        &mut engine,
        "/proj/t.ts",
        vec![],
        vec![doc("t1", "/proj/t.ts", r#"<button aria-pressd="x">y</button>"#)],
    );
    let value = engine.query(serde_json::from_value(serde_json::json!({
        "type": "codeFixes", "documentId": "t1", "start": 8, "end": 19
    })).unwrap());
    let fixes = value.as_array().unwrap();
    assert!(!fixes.is_empty());
    assert!(fixes[0]["label"].as_str().unwrap().contains("aria-pressed"));
}

#[test]
fn document_info_reports_placeholder_and_directive_arg() {
    let mut engine = strict_engine();
    let source = "<input ${ref('findInput')} />";
    let arg = source.find("findInput").unwrap() as u32;
    let mut template = doc("t1", "/proj/t.ts", source);
    template.placeholders[0].expr = Some(directive("ref", Some(("findInput", arg, arg + 9))));
    upsert(&mut engine, "/proj/t.ts", vec![], vec![template]);
    let value = engine.query(serde_json::from_value(serde_json::json!({
        "type": "documentInfoAt", "documentId": "t1", "offset": arg + 2
    })).unwrap());
    assert_eq!(value["inPlaceholder"], 0);
    assert_eq!(value["directiveName"], "ref");
}

#[test]
fn utf16_offsets_survive_non_ascii() {
    let mut engine = strict_engine();
    // The title contains ⌘ (1 UTF-16 unit, 3 UTF-8 bytes) before a bad attr.
    let source = r#"<button title="⌘K" aria-pressd="x">y</button>"#;
    upsert(
        &mut engine,
        "/proj/t.ts",
        vec![],
        vec![doc("t1", "/proj/t.ts", source)],
    );
    let result = engine.analyze("t1").unwrap();
    assert_eq!(rule_ids(&result), vec!["no-unknown-attribute"]);
    let d = &result.diagnostics[0];
    // In UTF-16 units, "aria-pressd" starts at the same index a JS string
    // would report.
    let js_index = source
        .chars()
        .take_while(|_| false)
        .count(); // placeholder to keep the explanation honest below
    let _ = js_index;
    let expected: u32 = {
        let prefix = &source[..source.find("aria-pressd").unwrap()];
        prefix.chars().map(|c| c.len_utf16() as u32).sum()
    };
    assert_eq!(d.start, expected);
}

#[test]
fn json_round_trip_through_the_string_boundary() {
    let mut engine = Engine::new();
    engine.set_config_json(r#"{"strict": true}"#).unwrap();
    let (text, placeholders) = substitute("<div>${(x) => x.a}</div>");
    let upsert = serde_json::json!({
        "fileName": "/proj/t.ts",
        "dependencies": [],
        "components": [],
        "documents": [{
            "id": "t1",
            "fileName": "/proj/t.ts",
            "templateStart": 10,
            "kind": "html",
            "text": text,
            "placeholders": placeholders.iter().map(|p| serde_json::json!({
                "index": p.index, "start": p.start, "end": p.end,
                "expr": {"kind": "arrow", "isFunctionType": true, "isConstant": false}
            })).collect::<Vec<_>>(),
        }]
    });
    engine.upsert_file_json(&upsert.to_string()).unwrap();
    let result = engine.analyze_json("t1").unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["diagnostics"].as_array().unwrap().len(), 0);
    let severities = engine
        .query_json(r#"{"type": "severities"}"#)
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&severities).unwrap();
    assert_eq!(parsed["no-unclosed-tag"], "error");
}
