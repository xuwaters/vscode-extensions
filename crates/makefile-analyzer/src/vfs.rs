//! Makefile workspace, built on the shared [`analyzer_core::vfs`] generics.
//!
//! Makefile analysis is single-file: we do not follow `include` directives
//! during parsing.

use analyzer_core::vfs::AnalyzerLang;

pub use analyzer_core::vfs::{FileUri, ParsedFile as CoreParsedFile};

pub struct MakefileLang;

impl AnalyzerLang for MakefileLang {
    type Ast = crate::ast::File;
    type Code = crate::diagnostics::DiagnosticCode;

    fn parse(uri: FileUri, source: String) -> CoreParsedFile<Self> {
        crate::parse::parse(uri, source)
    }
}

pub type Workspace = analyzer_core::vfs::Workspace<MakefileLang>;
pub type ParsedFile = analyzer_core::vfs::ParsedFile<MakefileLang>;
