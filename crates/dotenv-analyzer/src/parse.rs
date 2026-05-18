//! Parse pipeline — thin facade over [`crate::vfs::DotenvLang`].

use analyzer_core::vfs::AnalyzerLang;

pub use crate::vfs::{DotenvLang, ParsedFile};
use crate::vfs::FileUri;

pub fn parse(uri: FileUri, source: String) -> ParsedFile {
    <DotenvLang as AnalyzerLang>::parse(uri, source)
}
