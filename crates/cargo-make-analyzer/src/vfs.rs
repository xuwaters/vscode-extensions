//! cargo-make workspace, built on the shared [`analyzer_core::vfs`]
//! generics. Analysis is single-file: `extend` references are not followed.

use analyzer_core::vfs::AnalyzerLang;

pub use analyzer_core::vfs::{FileUri, ParsedFile as CoreParsedFile};

pub struct CargoMakeLang;

impl AnalyzerLang for CargoMakeLang {
    type Ast = crate::ast::File;
    type Code = crate::diagnostics::DiagnosticCode;

    fn parse(uri: FileUri, source: String) -> CoreParsedFile<Self> {
        crate::parse::parse(uri, source)
    }
}

pub type Workspace = analyzer_core::vfs::Workspace<CargoMakeLang>;
pub type ParsedFile = analyzer_core::vfs::ParsedFile<CargoMakeLang>;
