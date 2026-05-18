//! Minimal virtual filesystem — built atop the shared
//! [`analyzer_core::vfs`] generics. Plugging in the dotenv parser is a
//! tiny `AnalyzerLang` impl below.

use crate::ast;
use crate::diagnostics::DiagnosticCode;
use crate::parser::Parser;
use crate::spans::SpanTable;
use analyzer_core::vfs::AnalyzerLang;

pub use analyzer_core::vfs::{FileUri, ParsedFile as CoreParsedFile};

pub struct DotenvLang;

impl AnalyzerLang for DotenvLang {
    type Ast = ast::File;
    type Code = DiagnosticCode;

    fn parse(uri: FileUri, source: String) -> CoreParsedFile<Self> {
        let spans = SpanTable::new(&source);
        let mut parser = Parser::new(&source);
        let ast = parser.parse_file();
        let diagnostics = parser.into_diagnostics();
        CoreParsedFile { uri, source, ast, spans, diagnostics }
    }
}

pub type Workspace = analyzer_core::vfs::Workspace<DotenvLang>;
pub type ParsedFile = analyzer_core::vfs::ParsedFile<DotenvLang>;
