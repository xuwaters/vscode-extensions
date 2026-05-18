//! Shared virtual-filesystem primitives.
//!
//! - [`FileUri`] is the opaque URI-like key used by every analyzer.
//! - [`AnalyzerLang`] is the trait a crate implements to plug its AST and
//!   diagnostic-code type into the generic [`Workspace`].
//! - [`ParsedFile`] is the per-file payload stored by the workspace.
//!
//! Analyzers with richer per-workspace state (proto3's import graph,
//! capnp's reverse-include map) keep their own `Workspace` type and just
//! adopt [`FileUri`] from here.

use crate::diagnostics::Diagnostic;
use crate::spans::SpanTable;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FileUri(pub String);

impl FileUri {
    pub fn new(uri: impl Into<String>) -> Self {
        FileUri(uri.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for FileUri {
    fn from(s: &str) -> Self {
        FileUri(s.to_string())
    }
}

impl From<String> for FileUri {
    fn from(s: String) -> Self {
        FileUri(s)
    }
}

/// What a per-language analyzer needs to provide so it can use the shared
/// [`Workspace`].
pub trait AnalyzerLang: 'static {
    type Ast;
    type Code: Copy;

    /// Parse a single source string into a [`ParsedFile`]. The function is
    /// called by [`Workspace::update_file`].
    fn parse(uri: FileUri, source: String) -> ParsedFile<Self>;
}

pub struct ParsedFile<L: AnalyzerLang + ?Sized> {
    pub uri: FileUri,
    pub source: String,
    pub ast: L::Ast,
    pub spans: SpanTable,
    pub diagnostics: Vec<Diagnostic<L::Code>>,
}

impl<L> Clone for ParsedFile<L>
where
    L: AnalyzerLang + ?Sized,
    L::Ast: Clone,
    L::Code: Clone,
{
    fn clone(&self) -> Self {
        ParsedFile {
            uri: self.uri.clone(),
            source: self.source.clone(),
            ast: self.ast.clone(),
            spans: self.spans.clone(),
            diagnostics: self.diagnostics.clone(),
        }
    }
}

impl<L> std::fmt::Debug for ParsedFile<L>
where
    L: AnalyzerLang + ?Sized,
    L::Ast: std::fmt::Debug,
    L::Code: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ParsedFile")
            .field("uri", &self.uri)
            .field("source", &self.source)
            .field("ast", &self.ast)
            .field("spans", &self.spans)
            .field("diagnostics", &self.diagnostics)
            .finish()
    }
}

pub struct Workspace<L: AnalyzerLang> {
    files: HashMap<FileUri, ParsedFile<L>>,
}

impl<L: AnalyzerLang> Workspace<L> {
    pub fn new() -> Self {
        Workspace { files: HashMap::new() }
    }

    pub fn update_file(&mut self, uri: FileUri, source: String) {
        let parsed = L::parse(uri.clone(), source);
        self.files.insert(uri, parsed);
    }

    pub fn remove_file(&mut self, uri: &FileUri) {
        self.files.remove(uri);
    }

    pub fn file(&self, uri: &FileUri) -> Option<&ParsedFile<L>> {
        self.files.get(uri)
    }

    pub fn diagnostics_for(&self, uri: &FileUri) -> Vec<Diagnostic<L::Code>>
    where
        L::Code: Clone,
    {
        self.files
            .get(uri)
            .map(|f| f.diagnostics.clone())
            .unwrap_or_default()
    }
}

impl<L: AnalyzerLang> Default for Workspace<L> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::{Diagnostic, DiagnosticCode, Severity};
    use crate::spans::ByteSpan;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    enum TestCode {
        Demo,
    }
    impl DiagnosticCode for TestCode {
        fn as_str(self) -> &'static str {
            "T001"
        }
    }

    #[derive(Debug, Clone)]
    struct TestAst {
        line_count: u32,
    }

    struct TestLang;
    impl AnalyzerLang for TestLang {
        type Ast = TestAst;
        type Code = TestCode;
        fn parse(uri: FileUri, source: String) -> ParsedFile<Self> {
            let spans = SpanTable::new(&source);
            let line_count = source.lines().count() as u32;
            let diagnostics = if source.is_empty() {
                vec![Diagnostic::new(TestCode::Demo, Severity::Warning, "empty", ByteSpan::EMPTY)]
            } else {
                vec![]
            };
            ParsedFile {
                uri,
                source,
                ast: TestAst { line_count },
                spans,
                diagnostics,
            }
        }
    }

    #[test]
    fn update_then_read() {
        let mut ws: Workspace<TestLang> = Workspace::new();
        let uri = FileUri::new("test://a");
        ws.update_file(uri.clone(), "one\ntwo".into());
        let pf = ws.file(&uri).expect("inserted");
        assert_eq!(pf.ast.line_count, 2);
        assert!(pf.diagnostics.is_empty());
    }

    #[test]
    fn remove_drops_entry() {
        let mut ws: Workspace<TestLang> = Workspace::new();
        let uri = FileUri::new("test://a");
        ws.update_file(uri.clone(), "x".into());
        ws.remove_file(&uri);
        assert!(ws.file(&uri).is_none());
    }

    #[test]
    fn diagnostics_for_missing_uri_is_empty() {
        let ws: Workspace<TestLang> = Workspace::new();
        assert!(ws.diagnostics_for(&FileUri::new("nope")).is_empty());
    }

    #[test]
    fn diagnostics_for_returns_clones() {
        let mut ws: Workspace<TestLang> = Workspace::new();
        let uri = FileUri::new("test://a");
        ws.update_file(uri.clone(), String::new());
        let diags = ws.diagnostics_for(&uri);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Warning);
        assert_eq!(diags[0].code.as_str(), "T001");
    }

    #[test]
    fn file_uri_from_str_and_string() {
        let a: FileUri = "foo".into();
        let b: FileUri = String::from("foo").into();
        assert_eq!(a, b);
        assert_eq!(a.as_str(), "foo");
    }
}
