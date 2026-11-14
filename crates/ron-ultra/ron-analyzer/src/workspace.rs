//! Per-file store: source, AST, span table and diagnostics.

use crate::ast::File;
use crate::diagnostics::Diagnostic;
use crate::parser;
use crate::spans::SpanTable;
use std::collections::HashMap;

pub use analyzer_core::vfs::FileUri;

#[derive(Debug, Clone)]
pub struct ParsedFile {
    pub uri: FileUri,
    pub source: String,
    pub ast: File,
    pub spans: SpanTable,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn parse(uri: FileUri, source: String) -> ParsedFile {
    let spans = SpanTable::new(&source);
    let (ast, diagnostics) = parser::parse(&source);
    ParsedFile { uri, source, ast, spans, diagnostics }
}

#[derive(Default)]
pub struct Workspace {
    files: HashMap<FileUri, ParsedFile>,
}

impl Workspace {
    pub fn new() -> Self {
        Workspace::default()
    }

    pub fn update_file(&mut self, uri: FileUri, source: String) {
        let parsed = parse(uri.clone(), source);
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
        let uri = FileUri::new("test://a.ron");
        ws.update_file(uri.clone(), "(a: 1)".into());
        assert!(ws.file(&uri).expect("stored").diagnostics.is_empty());
        ws.update_file(uri.clone(), "(a: 1".into());
        assert_eq!(ws.file(&uri).unwrap().diagnostics.len(), 1);
        ws.remove_file(&uri);
        assert!(ws.file(&uri).is_none());
    }
}
