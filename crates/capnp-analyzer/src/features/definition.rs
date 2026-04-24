//! Go-to-definition. Resolves the type reference at `offset` to a symbol
//! and returns its declaration location. Falls back to the enclosing file
//! for bare file aliases (`Cxx` in `Cxx.foo`).

use super::position::type_use_at;
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
    let site = type_use_at(file, offset)?;
    match index.resolve_type_with_import(
        uri,
        site.enclosing_scope.as_str(),
        site.import_path.as_deref(),
        &site.path,
    ) {
        Resolution::Found { symbol, .. } => Some(Location {
            file: symbol.file.as_str().into(),
            range: symbol.name_span,
        }),
        Resolution::FileAlias { file, .. } => Some(Location {
            file: file.as_str().into(),
            range: ByteSpan::EMPTY,
        }),
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
        let src = "@0x1; struct Foo { id @0 :UInt32; } struct Bar { f @0 :Foo; }";
        ws.update("file:///a.capnp", src.into());
        let idx = WorkspaceIndex::build(&ws);
        let uri = FileUri("file:///a.capnp".into());
        // Cursor sits on the `Foo` inside `:Foo`.
        let offset = offset_of(src, ":Foo") + 1;
        let loc = definition(&ws, &idx, &uri, offset).expect("expected a definition");
        assert_eq!(loc.file, "file:///a.capnp");
    }

    #[test]
    fn jump_through_using_import_alias() {
        let mut ws = Workspace::new();
        ws.update("file:///a.capnp", "@0x1; struct Foo { id @0 :UInt32; }".into());
        let b = "@0x2; using Foo = import \"a.capnp\".Foo; struct Bar { f @0 :Foo; }";
        ws.update("file:///b.capnp", b.into());
        let idx = WorkspaceIndex::build(&ws);
        let uri = FileUri("file:///b.capnp".into());
        // Cursor on `:Foo` inside `Bar`.
        let offset = offset_of(b, ":Foo") + 1;
        let loc = definition(&ws, &idx, &uri, offset).expect("expected a definition");
        assert_eq!(loc.file, "file:///a.capnp");
    }

    #[test]
    fn jump_to_inline_import_type() {
        let mut ws = Workspace::new();
        ws.update("file:///a.capnp", "@0x1; struct Foo { id @0 :UInt32; }".into());
        let b = "@0x2; struct Bar { f @0 :import \"a.capnp\".Foo; }";
        ws.update("file:///b.capnp", b.into());
        let idx = WorkspaceIndex::build(&ws);
        let uri = FileUri("file:///b.capnp".into());
        let offset = offset_of(b, ".Foo") + 1;
        let loc = definition(&ws, &idx, &uri, offset).expect("expected a definition");
        assert_eq!(loc.file, "file:///a.capnp");
    }
}
