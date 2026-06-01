//! End-to-end workspace tests over a small multi-file project modelled on the
//! upstream `mojom-language-support` test fixtures: two mutually-importing
//! interface files plus a module-scoped struct in a subdirectory.

use mojom_analyzer::diagnostics::workspace_diagnostics;
use mojom_analyzer::features::definition::definition;
use mojom_analyzer::features::symbols::document_symbols;
use mojom_analyzer::resolve::WorkspaceIndex;
use mojom_analyzer::spans::{LineCol, SpanTable};
use mojom_analyzer::vfs::{FileUri, Workspace};

const MY_INTERFACE: &str = r#"import "my_service.mojom";

// MyInterface is my own interface.
interface MyInterface {
    enum MyInnerEnum { kOne, kTwo, KThree, };

    GetService() => (MyService service);
    DoSomething(BarStruct bar);
};
"#;

const MY_SERVICE: &str = r#"import "my_interface.mojom";
import "foo_module/foo.mojom";

interface MyService {
    GetMyInterface() => (MyInterface my_interface);
};
"#;

const FOO: &str = r#"module foo;

struct FooStruct {
    enum FooEnum { kOne, kTwo, kThree, };
};
"#;

fn project() -> Workspace {
    let mut ws = Workspace::new();
    ws.update("file:///proj/my_interface.mojom", MY_INTERFACE.into());
    ws.update("file:///proj/my_service.mojom", MY_SERVICE.into());
    ws.update("file:///proj/foo_module/foo.mojom", FOO.into());
    ws
}

fn line_col_offset(source: &str, needle: &str, extra: usize) -> (u32, u32) {
    let byte = source.find(needle).expect("needle") + extra;
    let table = SpanTable::new(source);
    let lc = table.offset_to_line_col(source, byte as u32);
    (lc.line, lc.col)
}

#[test]
fn imported_type_resolves_but_unknown_type_is_flagged() {
    let ws = project();
    let index = WorkspaceIndex::build(&ws);
    let uri = FileUri("file:///proj/my_interface.mojom".into());
    let diags = workspace_diagnostics(&ws, &index, &uri);

    // BarStruct is referenced but never declared anywhere.
    assert!(
        diags.iter().any(|d| d.code == "MOJOM0030" && d.message.contains("BarStruct")),
        "expected unknown-type diagnostic for BarStruct, got {diags:?}",
    );
    // MyService is declared in the imported my_service.mojom — no diagnostic.
    assert!(
        !diags.iter().any(|d| d.message.contains("MyService")),
        "MyService should resolve cleanly, got {diags:?}",
    );
}

#[test]
fn definition_jumps_across_import() {
    let ws = project();
    let index = WorkspaceIndex::build(&ws);
    let uri = FileUri("file:///proj/my_interface.mojom".into());
    let table = SpanTable::new(MY_INTERFACE);
    // Cursor on `MyService` inside the response list.
    let (line, col) = line_col_offset(MY_INTERFACE, "MyService service", 2);
    let offset = table.line_col_to_offset(MY_INTERFACE, LineCol { line, col });
    let loc = definition(&ws, &index, &uri, offset).expect("definition");
    assert_eq!(loc.file, "file:///proj/my_service.mojom");
}

#[test]
fn module_struct_outline_has_nested_enum() {
    let ws = project();
    let foo = ws.get("file:///proj/foo_module/foo.mojom").unwrap();
    let syms = document_symbols(&foo.analysis.file);
    // module foo + struct FooStruct
    assert!(syms.iter().any(|s| s.name == "foo"));
    let foo_struct = syms.iter().find(|s| s.name == "FooStruct").expect("FooStruct");
    assert!(foo_struct.children.iter().any(|c| c.name == "FooEnum"));
}

#[test]
fn removing_a_file_drops_its_symbols() {
    let mut ws = project();
    ws.remove("file:///proj/foo_module/foo.mojom");
    let index = WorkspaceIndex::build(&ws);
    assert!(index.lookup("foo.FooStruct").is_none());
}
