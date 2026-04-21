//! Phase 3 tests: find-references + rename.

use proto3_analyzer::features::{references, rename};
use proto3_analyzer::resolve::{ReferenceIndex, WorkspaceIndex};
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
fn find_references_across_files() {
    let ws = ws_with(&[
        (
            "test://base.proto",
            r#"syntax = "proto3";
package demo;
message Target { int32 n = 1; }
"#,
        ),
        (
            "test://a.proto",
            r#"syntax = "proto3";
package demo;
import "base.proto";
message Box { Target t = 1; }
"#,
        ),
        (
            "test://b.proto",
            r#"syntax = "proto3";
package demo;
import "base.proto";
message Bag { repeated Target items = 1; }
"#,
        ),
    ]);
    let uri = FileUri::new("test://base.proto");
    let pf = ws.file(&uri).unwrap();
    // Position on the `Target` definition name.
    let line_text = pf.source.lines().nth(2).unwrap();
    let col = line_text.find("Target").unwrap() as u32 + 1;
    let offset = pf.spans.line_col_to_offset(&pf.source, LineCol { line: 2, col });

    // references requires cursor on a use-site, not a def. Query from a.proto.
    let a_uri = FileUri::new("test://a.proto");
    let a_pf = ws.file(&a_uri).unwrap();
    let a_line = a_pf.source.lines().nth(3).unwrap();
    let a_col = a_line.find("Target").unwrap() as u32 + 1;
    let a_offset = a_pf.spans.line_col_to_offset(&a_pf.source, LineCol { line: 3, col: a_col });

    let index = WorkspaceIndex::build(&ws);
    let ref_index = ReferenceIndex::build(&ws, &index);
    let refs = references::references(&ws, &index, &ref_index, &a_uri, a_offset, true);
    let files: Vec<_> = refs.iter().map(|r| r.file.as_str()).collect();
    assert!(files.contains(&"test://a.proto"));
    assert!(files.contains(&"test://b.proto"));
    assert!(files.contains(&"test://base.proto"), "expected declaration in results: {:?}", files);
    let _ = offset;
}

#[test]
fn rename_message_across_files() {
    let ws = ws_with(&[
        (
            "test://base.proto",
            r#"syntax = "proto3";
package demo;
message Widget { int32 n = 1; }
"#,
        ),
        (
            "test://use.proto",
            r#"syntax = "proto3";
package demo;
import "base.proto";
message Box { Widget w = 1; repeated Widget many = 2; }
"#,
        ),
    ]);
    let uri = FileUri::new("test://use.proto");
    let pf = ws.file(&uri).unwrap();
    let line = pf.source.lines().nth(3).unwrap();
    let col = line.find("Widget").unwrap() as u32 + 1;
    let offset = pf.spans.line_col_to_offset(&pf.source, LineCol { line: 3, col });

    let index = WorkspaceIndex::build(&ws);
    let edit = rename::rename(&ws, &index, &uri, offset, "Thingy").expect("edit");

    // Definition site in base.proto
    let base_edits = edit.changes.get("test://base.proto").expect("base edits");
    assert_eq!(base_edits.len(), 1);
    assert_eq!(base_edits[0].new_text, "Thingy");

    // Two references in use.proto
    let use_edits = edit.changes.get("test://use.proto").expect("use edits");
    assert_eq!(use_edits.len(), 2);
    assert!(use_edits.iter().all(|e| e.new_text == "Thingy"));

    // Descending by start so appliers can edit in place
    assert!(use_edits[0].range.start > use_edits[1].range.start);
}

#[test]
fn rename_rejects_invalid_identifier() {
    let ws = ws_with(&[(
        "test://a.proto",
        r#"syntax = "proto3";
message A {}
"#,
    )]);
    let uri = FileUri::new("test://a.proto");
    let pf = ws.file(&uri).unwrap();
    let line = pf.source.lines().nth(1).unwrap();
    let col = line.find('A').unwrap() as u32 + 1;
    let offset = pf.spans.line_col_to_offset(&pf.source, LineCol { line: 1, col });
    let index = WorkspaceIndex::build(&ws);
    assert!(rename::rename(&ws, &index, &uri, offset, "1Bad").is_none());
    assert!(rename::rename(&ws, &index, &uri, offset, "has space").is_none());
}

#[test]
fn prepare_rename_returns_tail_span_for_qualified_ref() {
    let ws = ws_with(&[
        (
            "test://base.proto",
            r#"syntax = "proto3";
package demo;
message Target {}
"#,
        ),
        (
            "test://use.proto",
            r#"syntax = "proto3";
package other;
import "base.proto";
message Box { demo.Target t = 1; }
"#,
        ),
    ]);
    let uri = FileUri::new("test://use.proto");
    let pf = ws.file(&uri).unwrap();
    let line = pf.source.lines().nth(3).unwrap();
    let col = line.find("Target").unwrap() as u32 + 1;
    let offset = pf.spans.line_col_to_offset(&pf.source, LineCol { line: 3, col });
    let index = WorkspaceIndex::build(&ws);
    let span = rename::prepare_rename(&ws, &index, &uri, offset).expect("prep");
    // The prep span should cover just `Target`, not `demo.Target`.
    let text = &pf.source[span.start as usize..span.end as usize];
    assert_eq!(text, "Target");
}
