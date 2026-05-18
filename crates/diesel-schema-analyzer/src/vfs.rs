//! Workspace built on the shared [`analyzer_core::vfs`] generics. Parsing
//! lives in [`crate::parse`] — `DieselLang::parse` just forwards there.

use analyzer_core::vfs::AnalyzerLang;

pub use analyzer_core::vfs::{FileUri, ParsedFile as CoreParsedFile};

pub struct DieselLang;

impl AnalyzerLang for DieselLang {
    type Ast = crate::ast::SchemaFile;
    type Code = crate::diagnostics::DiagnosticCode;

    fn parse(uri: FileUri, source: String) -> CoreParsedFile<Self> {
        crate::parse::parse(uri, source)
    }
}

pub type Workspace = analyzer_core::vfs::Workspace<DieselLang>;
pub type ParsedFile = analyzer_core::vfs::ParsedFile<DieselLang>;
