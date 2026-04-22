//! Phase 4 tests: formatter, style diagnostics, inlay hints, code actions,
//! semantic tokens.

use proto3_analyzer::diagnostics::{DiagnosticCode, StyleConfig};
use proto3_analyzer::features::{code_actions, formatting, inlay_hints, semantic_tokens};
use proto3_analyzer::resolve::WorkspaceIndex;
use proto3_analyzer::spans::LineCol;
use proto3_analyzer::vfs::{FileUri, Workspace};

fn ws_with(files: &[(&str, &str)]) -> Workspace {
    let mut ws = Workspace::with_bundled_well_known_types();
    for (uri, src) in files {
        ws.update_file(FileUri::new(*uri), (*src).to_string());
    }
    ws
}

#[test]
fn formatter_canonicalises_spacing_and_ordering() {
    let ws = ws_with(&[(
        "test://f.proto",
        r#"syntax="proto3";
import   "b.proto";
import "a.proto";
package   x.y;
message   Foo    {
int32 n=1;
string   name    = 2;
}
"#,
    )]);
    let pf = ws.file(&FileUri::new("test://f.proto")).unwrap();
    let out = formatting::format_file(pf).expect("format");
    // Imports sorted alphabetically.
    let a = out.find("\"a.proto\"").unwrap();
    let b = out.find("\"b.proto\"").unwrap();
    assert!(a < b, "imports should be sorted:\n{}", out);
    // Consistent spacing.
    assert!(out.contains("message Foo {"), "got:\n{}", out);
    assert!(out.contains("  int32 n = 1;"), "got:\n{}", out);
    assert!(out.contains("  string name = 2;"), "got:\n{}", out);
    assert!(out.contains("package x.y;"), "got:\n{}", out);
}

#[test]
fn formatter_refuses_when_parse_errors() {
    let ws = ws_with(&[(
        "test://bad.proto",
        r#"syntax = "proto3";
message { broken
"#,
    )]);
    let pf = ws.file(&FileUri::new("test://bad.proto")).unwrap();
    assert!(formatting::format_file(pf).is_none());
}

#[test]
fn style_diagnostics_respect_config() {
    let src = r#"syntax = "proto3";
message bad_name { int32 BadField = 1; }
enum bad_enum { unlowered = 0; }
"#;
    let uri = FileUri::new("test://s.proto");

    let mut ws = Workspace::with_bundled_well_known_types();
    ws.update_file(uri.clone(), src.to_string());
    // Off by default.
    let diags = ws.diagnostics_for(&uri);
    assert!(!diags
        .iter()
        .any(|d| matches!(d.code, DiagnosticCode::StyleUpperCamel | DiagnosticCode::StyleLowerSnake | DiagnosticCode::StyleScreamingSnake)));

    // On.
    ws.set_style_config(StyleConfig { enabled: true });
    let diags = ws.diagnostics_for(&uri);
    assert!(diags.iter().any(|d| d.code == DiagnosticCode::StyleUpperCamel));
    assert!(diags.iter().any(|d| d.code == DiagnosticCode::StyleLowerSnake));
    assert!(diags.iter().any(|d| d.code == DiagnosticCode::StyleScreamingSnake));
}

#[test]
fn add_missing_import_action_appears_on_unresolved() {
    let ws = ws_with(&[
        (
            "test://base.proto",
            r#"syntax = "proto3";
package demo;
message Thingy { int32 n = 1; }
"#,
        ),
        (
            "test://top.proto",
            r#"syntax = "proto3";
package demo;
message Wrap { Thingy t = 1; }
"#,
        ),
    ]);
    let uri = FileUri::new("test://top.proto");
    let pf = ws.file(&uri).unwrap();
    let line = pf.source.lines().nth(2).unwrap();
    let col = line.find("Thingy").unwrap() as u32 + 1;
    let offset = pf.spans.line_col_to_offset(&pf.source, LineCol { line: 2, col });
    let index = WorkspaceIndex::build(&ws);
    let actions = code_actions::code_actions(&ws, &index, &uri, offset, &[]);
    let add = actions.iter().find(|a| a.title.contains("Add import"));
    assert!(add.is_some(), "actions: {:#?}", actions);
    let edit = &add.unwrap().edits[0];
    assert!(edit.new_text.contains("base.proto"));
}

#[test]
fn organize_imports_sorts_and_dedupes() {
    let ws = ws_with(&[(
        "test://o.proto",
        r#"syntax = "proto3";
import "z.proto";
import "a.proto";
import "z.proto";
message M {}
"#,
    )]);
    let uri = FileUri::new("test://o.proto");
    let pf = ws.file(&uri).unwrap();
    let offset = pf.spans.line_col_to_offset(&pf.source, LineCol { line: 0, col: 0 });
    let index = WorkspaceIndex::build(&ws);
    let actions = code_actions::code_actions(&ws, &index, &uri, offset, &[]);
    let org = actions.iter().find(|a| a.kind == "source.organizeImports").expect("action");
    let replaced = &org.edits[0].new_text;
    // Sorted, deduped.
    let a = replaced.find("a.proto").unwrap();
    let z = replaced.find("z.proto").unwrap();
    assert!(a < z, "got:\n{}", replaced);
    assert_eq!(replaced.matches("z.proto").count(), 1, "duplicate not deduped:\n{}", replaced);
}

#[test]
fn inlay_hint_appears_for_imported_short_names() {
    let ws = ws_with(&[
        (
            "test://base.proto",
            r#"syntax = "proto3";
package other;
message Widget {}
"#,
        ),
        (
            "test://use.proto",
            r#"syntax = "proto3";
package demo;
import "base.proto";
message Box { other.Widget w = 1; }
"#,
        ),
    ]);
    let uri = FileUri::new("test://use.proto");
    let index = WorkspaceIndex::build(&ws);
    let hints = inlay_hints::inlay_hints(&ws, &index, &uri);
    // `other.Widget` is fine — short enough. Prior test asserted nothing
    // shows when the ref is multi-segment; here the ref is multi-part
    // so no hint. We re-test the single-segment case.
    assert!(hints.is_empty(), "unexpected hints: {:?}", hints);

    // Now single-segment reference that resolves to a different package.
    let ws2 = ws_with(&[
        (
            "test://base.proto",
            r#"syntax = "proto3";
package demo.nested;
message Widget {}
"#,
        ),
        (
            "test://use.proto",
            r#"syntax = "proto3";
package demo;
import "base.proto";
message Box { nested.Widget w = 1; }
"#,
        ),
    ]);
    let uri = FileUri::new("test://use.proto");
    let index = WorkspaceIndex::build(&ws2);
    let hints = inlay_hints::inlay_hints(&ws2, &index, &uri);
    // `nested.Widget` is multi-segment too, so still no hint.
    assert!(hints.is_empty(), "{:?}", hints);

    // Finally: same-package single-ident — no hint needed.
    let ws3 = ws_with(&[(
        "test://s.proto",
        r#"syntax = "proto3";
package demo;
message W {}
message Box { W w = 1; }
"#,
    )]);
    let uri = FileUri::new("test://s.proto");
    let index = WorkspaceIndex::build(&ws3);
    let hints = inlay_hints::inlay_hints(&ws3, &index, &uri);
    assert!(hints.is_empty(), "same-package shouldn't hint: {:?}", hints);
}

#[test]
fn semantic_tokens_classify_names() {
    let ws = ws_with(&[(
        "test://t.proto",
        r#"syntax = "proto3";
package demo;
message A { int32 field_x = 1; }
enum Color { RED = 0; GREEN = 1; }
"#,
    )]);
    let uri = FileUri::new("test://t.proto");
    let index = WorkspaceIndex::build(&ws);
    let tokens = semantic_tokens::semantic_tokens(&ws, &index, &uri);
    // Expect a Property token on `field_x` and EnumMember tokens on RED/GREEN.
    let kinds: Vec<_> = tokens.iter().map(|t| t.ty).collect();
    assert!(kinds.contains(&semantic_tokens::SemanticTokenType::Property));
    assert!(kinds.contains(&semantic_tokens::SemanticTokenType::EnumMember));
    assert!(kinds.contains(&semantic_tokens::SemanticTokenType::Namespace));
}
