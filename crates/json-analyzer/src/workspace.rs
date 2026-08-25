//! Per-file store. This crate keeps its own `Workspace` (rather than
//! `analyzer_core::vfs::Workspace`) because a JSON file's parse depends
//! on its *flavor*, which the editor reports as a language id and the
//! generic workspace has no slot for.

use crate::ast::Ast;
use crate::diagnostics::Diagnostic;
use crate::flavor::Flavor;
use crate::parser::parse_document;
use crate::spans::SpanTable;
use std::collections::HashMap;

pub use analyzer_core::vfs::FileUri;

#[derive(Debug, Clone)]
pub struct ParsedFile {
    pub uri: FileUri,
    pub source: String,
    pub flavor: Flavor,
    pub ast: Ast,
    pub spans: SpanTable,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn parse(uri: FileUri, source: String, flavor: Flavor) -> ParsedFile {
    let spans = SpanTable::new(&source);
    let (ast, diagnostics) = parse_document(&source, flavor);
    ParsedFile { uri, source, flavor, ast, spans, diagnostics }
}

#[derive(Default)]
pub struct Workspace {
    files: HashMap<FileUri, ParsedFile>,
}

impl Workspace {
    pub fn new() -> Self {
        Workspace { files: HashMap::new() }
    }

    pub fn update_file(&mut self, uri: FileUri, source: String, flavor: Flavor) {
        let parsed = parse(uri.clone(), source, flavor);
        self.files.insert(uri, parsed);
    }

    pub fn remove_file(&mut self, uri: &FileUri) {
        self.files.remove(uri);
    }

    pub fn file(&self, uri: &FileUri) -> Option<&ParsedFile> {
        self.files.get(uri)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_then_read_then_remove() {
        let mut ws = Workspace::new();
        let uri = FileUri::new("test://a.json");
        ws.update_file(uri.clone(), "{\"a\": 1}".into(), Flavor::Json);
        let pf = ws.file(&uri).expect("stored");
        assert_eq!(pf.flavor, Flavor::Json);
        assert!(pf.diagnostics.is_empty());
        ws.remove_file(&uri);
        assert!(ws.file(&uri).is_none());
    }
}
