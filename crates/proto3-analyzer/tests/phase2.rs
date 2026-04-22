//! Phase 2 tests: cross-file resolution, definition, hover, completion,
//! unresolved-type and unused-import diagnostics.

use proto3_analyzer::diagnostics::DiagnosticCode;
use proto3_analyzer::features::{completion, definition, hover};
use proto3_analyzer::resolve::{Resolution, WorkspaceIndex};
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
fn resolve_type_in_same_package() {
    let ws = ws_with(&[(
        "test://a.proto",
        r#"
syntax = "proto3";
package demo;
message A { int32 x = 1; }
message B { A a = 1; }
"#,
    )]);
    let index = WorkspaceIndex::build(&ws);
    let pf = ws.file(&FileUri::new("test://a.proto")).unwrap();
    let sites = proto3_analyzer::resolve::collect_type_use_sites(&pf.ast);
    let site = sites.iter().find(|s| s.name.to_display() == "A").unwrap();
    let res = index.resolve_type(&FileUri::new("test://a.proto"), "demo.B", &site.name);
    assert!(matches!(res, Resolution::Found { .. }));
}

#[test]
fn cross_file_resolution_via_import() {
    let ws = ws_with(&[
        (
            "test://base.proto",
            r#"
syntax = "proto3";
package demo;
message Base { int32 id = 1; }
"#,
        ),
        (
            "test://user.proto",
            r#"
syntax = "proto3";
package demo;
import "base.proto";
message User { Base base = 1; }
"#,
        ),
    ]);
    let diags = ws.diagnostics_for(&FileUri::new("test://user.proto"));
    let errors: Vec<_> = diags
        .iter()
        .filter(|d| matches!(d.code, DiagnosticCode::UnknownType | DiagnosticCode::ImportUnresolved))
        .collect();
    assert!(errors.is_empty(), "unexpected: {:#?}", errors);
}

#[test]
fn unresolved_type_produces_diagnostic_with_suggestion() {
    let ws = ws_with(&[(
        "test://a.proto",
        r#"
syntax = "proto3";
package demo;
message Widget {}
message Gadget { Widgets thing = 1; }
"#,
    )]);
    let diags = ws.diagnostics_for(&FileUri::new("test://a.proto"));
    let unk: Vec<_> = diags.iter().filter(|d| d.code == DiagnosticCode::UnknownType).collect();
    assert_eq!(unk.len(), 1, "diags: {:#?}", diags);
    assert!(unk[0].message.contains("Widget"), "msg was: {}", unk[0].message);
}

#[test]
fn unused_import_warning() {
    let ws = ws_with(&[
        (
            "test://util.proto",
            r#"
syntax = "proto3";
package demo;
message Util {}
"#,
        ),
        (
            "test://main.proto",
            r#"
syntax = "proto3";
package demo;
import "util.proto";
message Main { int32 n = 1; }
"#,
        ),
    ]);
    let diags = ws.diagnostics_for(&FileUri::new("test://main.proto"));
    assert!(diags.iter().any(|d| d.code == DiagnosticCode::ImportUnused), "{:#?}", diags);
}

#[test]
fn definition_jumps_to_defining_file() {
    let ws = ws_with(&[
        (
            "test://base.proto",
            r#"syntax = "proto3";
package demo;
message Target { int32 n = 1; }
"#,
        ),
        (
            "test://user.proto",
            // `Target` appears on line 3 (0-indexed); `Box`'s scope is `demo`,
            // so unqualified `Target` resolves via scope walk to `demo.Target`.
            r#"syntax = "proto3";
package demo;
import "base.proto";
message Box { Target t = 1; }
"#,
        ),
    ]);
    let uri = FileUri::new("test://user.proto");
    let pf = ws.file(&uri).unwrap();
    // "message Box { Target" — T is at column 14 of line 3 (0-indexed).
    let offset = pf.spans.line_col_to_offset(&pf.source, LineCol { line: 3, col: 16 });
    let index = WorkspaceIndex::build(&ws);
    let loc = definition::definition(&ws, &index, &uri, offset).expect("resolution");
    assert_eq!(loc.file, "test://base.proto");
}

#[test]
fn hover_on_field_shows_label_type_number() {
    let ws = ws_with(&[(
        "test://h.proto",
        r#"syntax = "proto3";
message M {
  /// doc comment
  repeated string names = 7;
}
"#,
    )]);
    let uri = FileUri::new("test://h.proto");
    let pf = ws.file(&uri).unwrap();
    // Column of `names` identifier on line index 3.
    let line_text = pf.source.lines().nth(3).unwrap();
    let col = line_text.find("names").unwrap() as u32 + 1;
    let offset = pf.spans.line_col_to_offset(&pf.source, LineCol { line: 3, col });
    let index = WorkspaceIndex::build(&ws);
    let h = hover::hover(&ws, &index, &uri, offset).expect("hover");
    assert!(h.markdown.contains("repeated"), "md: {}", h.markdown);
    assert!(h.markdown.contains("names"), "md: {}", h.markdown);
    assert!(h.markdown.contains("= 7"), "md: {}", h.markdown);
}

#[test]
fn completion_offers_keywords_scalars_and_symbols() {
    let ws = ws_with(&[(
        "test://c.proto",
        r#"syntax = "proto3";
package demo;
message Foo {}
message Bar { int32 x = 1; }
"#,
    )]);
    let uri = FileUri::new("test://c.proto");
    let pf = ws.file(&uri).unwrap();
    let offset = pf.spans.line_col_to_offset(&pf.source, LineCol { line: 0, col: 0 });
    let index = WorkspaceIndex::build(&ws);
    let items = completion::completion(&ws, &index, &uri, offset);
    let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"message"), "want `message` keyword");
    assert!(labels.contains(&"int32"), "want `int32` scalar");
    assert!(labels.contains(&"Foo"), "want `Foo` from this file");
    assert!(labels.contains(&"Timestamp"), "want WKT `Timestamp`");
}

#[test]
fn duplicate_fqn_in_local_file_does_not_report_unknown_type() {
    // Both files define `demo.Shared`. `main.proto` does not import
    // `other.proto`, but it still has its own `Shared` — the resolver must
    // pick the local copy and not flag the use as missing-import.
    let ws = ws_with(&[
        (
            "test://other.proto",
            r#"syntax = "proto3";
package demo;
message Shared { int32 a = 1; }
"#,
        ),
        (
            "test://main.proto",
            r#"syntax = "proto3";
package demo;
message Shared { int32 b = 1; }
message User { Shared s = 1; }
"#,
        ),
    ]);
    let diags = ws.diagnostics_for(&FileUri::new("test://main.proto"));
    let unk: Vec<_> = diags
        .iter()
        .filter(|d| d.code == DiagnosticCode::UnknownType)
        .collect();
    assert!(unk.is_empty(), "unexpected: {:#?}", diags);
}

#[test]
fn public_import_chains_transit_visibility() {
    let ws = ws_with(&[
        (
            "test://inner.proto",
            r#"syntax = "proto3";
package demo;
message Inner {}
"#,
        ),
        (
            "test://reexport.proto",
            r#"syntax = "proto3";
package demo;
import public "inner.proto";
"#,
        ),
        (
            "test://top.proto",
            r#"syntax = "proto3";
package demo;
import "reexport.proto";
message User { Inner inner = 1; }
"#,
        ),
    ]);
    let diags = ws.diagnostics_for(&FileUri::new("test://top.proto"));
    let errors: Vec<_> = diags
        .iter()
        .filter(|d| d.severity == proto3_analyzer::diagnostics::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected: {:#?}", errors);
}
