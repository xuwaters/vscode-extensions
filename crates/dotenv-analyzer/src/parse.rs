//! The parse pipeline — parse a single source string into a [`ParsedFile`].

use crate::ast;
use crate::diagnostics::DotenvDiagnostic;
use crate::parser::Parser;
use crate::spans::SpanTable;
use crate::vfs::FileUri;

#[derive(Debug, Clone)]
pub struct ParsedFile {
    pub uri: FileUri,
    pub source: String,
    pub ast: ast::File,
    pub spans: SpanTable,
    pub diagnostics: Vec<DotenvDiagnostic>,
}

pub fn parse(uri: FileUri, source: String) -> ParsedFile {
    let spans = SpanTable::new(&source);
    let mut parser = Parser::new(&source);
    let ast = parser.parse_file();
    let diagnostics = parser.into_diagnostics();
    ParsedFile { uri, source, ast, spans, diagnostics }
}
