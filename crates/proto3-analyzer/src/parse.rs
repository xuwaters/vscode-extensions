//! The parse pipeline — lex then parse a single source into a [`ParsedFile`].

use crate::ast;
use crate::diagnostics::ProtoDiagnostic;
use crate::parser::Parser;
use crate::spans::SpanTable;
use crate::vfs::FileUri;

/// The canonical parsed representation for a single `.proto` file.
#[derive(Debug, Clone)]
pub struct ParsedFile {
    pub uri: FileUri,
    pub source: String,
    pub ast: ast::File,
    pub spans: SpanTable,
    pub diagnostics: Vec<ProtoDiagnostic>,
}

impl ParsedFile {
    pub fn imports(&self) -> impl Iterator<Item = &str> {
        self.ast.imports.iter().map(|i| i.path.as_str())
    }
}

pub fn parse(uri: FileUri, source: String) -> ParsedFile {
    let tokens = crate::lexer::lex(&source);
    let spans = SpanTable::new(&source);
    let mut parser = Parser::new(&source, tokens);
    let ast = parser.parse_file();
    let diagnostics = parser.into_diagnostics();
    ParsedFile { uri, source, ast, spans, diagnostics }
}
