//! Go-to-definition. Resolves the type reference (or import path) at `offset`
//! and returns its declaration location.

use super::position::{import_at, type_use_at};
use crate::resolve::{Resolution, WorkspaceIndex};
use crate::spans::ByteSpan;
use crate::vfs::{FileUri, Workspace};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Location {
    pub file: String,
    pub range: ByteSpan,
}

pub fn definition(
    ws: &Workspace,
    index: &WorkspaceIndex,
    uri: &FileUri,
    offset: u32,
) -> Option<Location> {
    let file = &ws.file(uri)?.analysis.file;

    // Cursor on an `import "…"` path → jump to the imported file.
    if let Some(path) = import_at(file, offset) {
        if let Some(target) = ws.resolve_import_path(uri, &path.value) {
            return Some(Location { file: target.as_str().into(), range: ByteSpan::EMPTY });
        }
        return None;
    }

    let site = type_use_at(file, offset)?;
    match index.resolve_type(uri, site.enclosing_scope.as_str(), &site.path) {
        Resolution::Found { symbol, .. } => {
            Some(Location { file: symbol.file.as_str().into(), range: symbol.name_span })
        }
        Resolution::Unknown { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve::WorkspaceIndex;
    use crate::vfs::Workspace;

    fn offset_of(src: &str, needle: &str) -> u32 {
        src.find(needle).expect("needle not found") as u32
    }

    #[test]
    fn jump_to_local_type() {
        let mut ws = Workspace::new();
        let src = "struct Foo { int32 id; };\nstruct Bar { Foo f; };";
        ws.update("file:///a.mojom", src.into());
        let idx = WorkspaceIndex::build(&ws);
        let uri = FileUri("file:///a.mojom".into());
        // Cursor on the `Foo` inside `Foo f;`.
        let offset = offset_of(src, "Foo f") + 1;
        let loc = definition(&ws, &idx, &uri, offset).expect("expected a definition");
        assert_eq!(loc.file, "file:///a.mojom");
    }

    #[test]
    fn jump_across_import() {
        let mut ws = Workspace::new();
        ws.update("file:///dep.mojom", "module dep;\nstruct Foo { int32 id; };".into());
        let main = "import \"dep.mojom\";\nstruct Bar { dep.Foo f; };";
        ws.update("file:///main.mojom", main.into());
        let idx = WorkspaceIndex::build(&ws);
        let uri = FileUri("file:///main.mojom".into());
        let offset = offset_of(main, "dep.Foo") + 5; // on "Foo"
        let loc = definition(&ws, &idx, &uri, offset).expect("expected a definition");
        assert_eq!(loc.file, "file:///dep.mojom");
    }

    #[test]
    fn jump_from_import_path() {
        let mut ws = Workspace::new();
        ws.update("file:///dep.mojom", "struct Foo { int32 id; };".into());
        let main = "import \"dep.mojom\";";
        ws.update("file:///main.mojom", main.into());
        let idx = WorkspaceIndex::build(&ws);
        let uri = FileUri("file:///main.mojom".into());
        let offset = offset_of(main, "dep.mojom") + 1;
        let loc = definition(&ws, &idx, &uri, offset).expect("expected a definition");
        assert_eq!(loc.file, "file:///dep.mojom");
    }

    #[test]
    fn jump_to_type_in_method_response() {
        let mut ws = Workspace::new();
        let src = "struct Out { int32 x; };\ninterface I { Foo() => (Out result); };";
        ws.update("file:///a.mojom", src.into());
        let idx = WorkspaceIndex::build(&ws);
        let uri = FileUri("file:///a.mojom".into());
        let offset = offset_of(src, "Out result") + 1;
        let loc = definition(&ws, &idx, &uri, offset).expect("expected a definition");
        assert_eq!(loc.file, "file:///a.mojom");
    }
}
