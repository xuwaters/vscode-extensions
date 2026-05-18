//! The parse pipeline — lex then parse a single source into a [`ParsedFile`].

use crate::lexer;
use crate::parser::Parser;
use crate::spans::SpanTable;
use crate::vfs::FileUri;

pub use crate::vfs::ParsedFile;

pub fn parse(uri: FileUri, source: String) -> ParsedFile {
    let lines = lexer::lex(&source);
    let spans = SpanTable::new(&source);
    let mut parser = Parser::new(&source, &lines);
    let ast = parser.parse_file();
    let diagnostics = parser.into_diagnostics();
    ParsedFile { uri, source, ast, spans, diagnostics }
}
