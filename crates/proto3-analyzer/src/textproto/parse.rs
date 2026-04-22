//! Entry point for parsing a textproto document. Produces a [`ParsedTextproto`]
//! bundle (AST + spans + parse-time diagnostics) that the workspace stores
//! alongside its `.proto` `ParsedFile` entries.

use super::ast::{File, HeaderHints};
use super::header;
use super::lexer::{lex, Comment};
use super::parser::parse_file;
use crate::diagnostics::ProtoDiagnostic;
use crate::spans::SpanTable;
use crate::vfs::FileUri;

/// Parsed textproto document. Mirrors the shape of [`crate::parse::ParsedFile`]
/// so workspace code can treat both uniformly where it doesn't care about
/// AST details.
#[derive(Debug, Clone)]
pub struct ParsedTextproto {
    pub uri: FileUri,
    pub source: String,
    pub ast: File,
    pub comments: Vec<Comment>,
    pub spans: SpanTable,
    /// Parse + header + lex diagnostics, ready to merge with schema checks.
    pub diagnostics: Vec<ProtoDiagnostic>,
}

impl ParsedTextproto {
    pub fn header(&self) -> &HeaderHints {
        &self.ast.header
    }
}

pub fn parse(uri: FileUri, source: String) -> ParsedTextproto {
    let spans = SpanTable::new(&source);
    let lex_out = lex(&source);
    let header_out = header::extract(&source, &lex_out.comments);
    let comments = lex_out.comments.clone();
    let mut parse_out = parse_file(&source, lex_out);
    parse_out.file.header = header_out.hints;

    let mut diagnostics = parse_out.diagnostics;
    diagnostics.extend(header_out.diagnostics);

    ParsedTextproto {
        uri,
        source,
        ast: parse_out.file,
        comments,
        spans,
        diagnostics,
    }
}
