//! Every LSP method the server answers.

use crate::convert::{diagnostic, offset, position, range, symbol_kind};
use crate::{Document, Server, Settings};
use lsp_types::{
    Command, CompletionItem, CompletionItemKind, CompletionParams, CompletionResponse,
    DidChangeConfigurationParams, DidChangeTextDocumentParams, DidChangeWatchedFilesParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DocumentFormattingParams, DocumentSymbol,
    DocumentSymbolParams, DocumentSymbolResponse, Documentation, FoldingRange, FoldingRangeKind,
    FoldingRangeParams, GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents, HoverParams,
    InsertTextFormat, Location, MarkupContent, MarkupKind, Range, SelectionRange, SelectionRangeParams,
    TextEdit, Uri,
};
use ron_analyzer::features::completion::{completions, CompletionKind};
use ron_analyzer::features::definition::definition;
use ron_analyzer::features::document_symbols::{document_symbols, DocumentSymbol as RonSymbol};
use ron_analyzer::features::folding::{folding_ranges, FoldKind};
use ron_analyzer::features::formatting::{format_file, FormatOptions};
use ron_analyzer::features::hover::hover;
use ron_analyzer::features::selection::selection_ranges;
use ron_analyzer::schema::{analyze, Schema};
use ron_analyzer::workspace::{parse, FileUri, ParsedFile};
use std::str::FromStr;

impl Server {
    // ---- document sync -----------------------------------------------------

    pub(crate) fn did_open(&mut self, p: DidOpenTextDocumentParams) {
        let uri = p.text_document.uri.as_str().to_string();
        self.store(uri.clone(), p.text_document.version, p.text_document.text);
        self.publish(&uri);
    }

    pub(crate) fn did_change(&mut self, p: DidChangeTextDocumentParams) {
        // Full sync: the last change carries the whole text.
        let Some(change) = p.content_changes.into_iter().last() else { return };
        let uri = p.text_document.uri.as_str().to_string();
        self.store(uri.clone(), p.text_document.version, change.text);
        self.publish(&uri);
    }

    pub(crate) fn did_close(&mut self, p: DidCloseTextDocumentParams) {
        let uri = p.text_document.uri.as_str().to_string();
        if self.documents.remove(&uri).is_some() {
            self.notify(
                "textDocument/publishDiagnostics",
                serde_json::json!({ "uri": uri, "diagnostics": [] }),
            );
        }
    }

    pub(crate) fn did_change_configuration(&mut self, p: DidChangeConfigurationParams) {
        // The client sends `{ settings: { ...ronUltra } }`; anything else
        // keeps the current settings.
        let Ok(settings) = serde_json::from_value::<Settings>(p.settings) else { return };
        if settings != self.settings {
            self.settings = settings;
            self.reanalyze_all();
        }
    }

    pub(crate) fn did_change_watched_files(&mut self, p: DidChangeWatchedFilesParams) {
        let mut changed = false;
        for change in p.changes {
            let uri = change.uri.as_str();
            if self.world.is_tracked(uri) {
                self.world.invalidate(uri);
                changed = true;
            }
        }
        if changed {
            self.reanalyze_all();
        }
    }

    fn store(&mut self, uri: String, version: i32, text: String) {
        let parsed = parse(FileUri::new(uri.clone()), text);
        let schema = self.schema_for(&parsed);
        self.documents.insert(uri, Document { version, parsed, schema });
    }

    fn schema_for(&self, parsed: &ParsedFile) -> Schema {
        if self.settings.schema.enabled { analyze(parsed, &self.world) } else { Schema::default() }
    }

    fn reanalyze_all(&mut self) {
        let uris: Vec<String> = self.documents.keys().cloned().collect();
        for uri in &uris {
            let schema = match self.documents.get(uri) {
                Some(doc) => self.schema_for(&doc.parsed),
                None => continue,
            };
            if let Some(doc) = self.documents.get_mut(uri) {
                doc.schema = schema;
            }
            self.publish(uri);
        }
    }

    fn publish(&mut self, uri: &str) {
        let Some(doc) = self.documents.get(uri) else { return };
        let diagnostics: Vec<lsp_types::Diagnostic> = if self.settings.diagnostics.enabled {
            let mut all: Vec<_> = doc.parsed.diagnostics.iter().chain(&doc.schema.diagnostics).collect();
            all.sort_by_key(|d| (d.span.start, d.span.end));
            all.into_iter().map(|d| diagnostic(&doc.parsed, d)).collect()
        } else {
            Vec::new()
        };
        let params = serde_json::json!({ "uri": uri, "version": doc.version, "diagnostics": diagnostics });
        self.notify("textDocument/publishDiagnostics", params);
    }

    // ---- reading -----------------------------------------------------------

    pub(crate) fn hover(&self, p: HoverParams) -> Option<Hover> {
        let doc = self.documents.get(p.text_document_position_params.text_document.uri.as_str())?;
        let pos = crate::convert::line_col(p.text_document_position_params.position);
        let h = hover(&doc.parsed, Some(&doc.schema), pos)?;
        Some(Hover {
            contents: HoverContents::Markup(MarkupContent { kind: MarkupKind::Markdown, value: h.contents }),
            range: Some(range(&doc.parsed, h.range)),
        })
    }

    pub(crate) fn completion(&self, p: CompletionParams) -> Option<CompletionResponse> {
        let doc = self.documents.get(p.text_document_position.text_document.uri.as_str())?;
        let at = offset(&doc.parsed, p.text_document_position.position);
        let items = completions(&doc.parsed, &doc.schema, &self.world, at)
            .into_iter()
            .map(|c| CompletionItem {
                label: c.label,
                kind: Some(match c.kind {
                    CompletionKind::Field => CompletionItemKind::FIELD,
                    CompletionKind::Variant => CompletionItemKind::ENUM_MEMBER,
                    CompletionKind::Struct => CompletionItemKind::STRUCT,
                    CompletionKind::Enum => CompletionItemKind::ENUM,
                    CompletionKind::Module => CompletionItemKind::MODULE,
                    CompletionKind::Keyword => CompletionItemKind::KEYWORD,
                    CompletionKind::Value => CompletionItemKind::VALUE,
                }),
                detail: c.detail,
                documentation: c
                    .documentation
                    .map(|d| Documentation::MarkupContent(MarkupContent { kind: MarkupKind::Markdown, value: d })),
                insert_text: c.insert_text,
                insert_text_format: Some(if c.snippet { InsertTextFormat::SNIPPET } else { InsertTextFormat::PLAIN_TEXT }),
                sort_text: c.sort_text,
                command: c.retrigger.then(|| Command {
                    title: "Suggest".into(),
                    command: "editor.action.triggerSuggest".into(),
                    arguments: None,
                }),
                ..CompletionItem::default()
            })
            .collect();
        Some(CompletionResponse::Array(items))
    }

    pub(crate) fn definition(&self, p: GotoDefinitionParams) -> Option<GotoDefinitionResponse> {
        let doc = self.documents.get(p.text_document_position_params.text_document.uri.as_str())?;
        let at = offset(&doc.parsed, p.text_document_position_params.position);
        let loc = definition(&doc.schema, at)?;
        Some(GotoDefinitionResponse::Scalar(Location {
            uri: Uri::from_str(&loc.uri).ok()?,
            range: Range { start: position(loc.start), end: position(loc.end) },
        }))
    }

    pub(crate) fn document_symbols(&self, p: DocumentSymbolParams) -> Option<DocumentSymbolResponse> {
        let doc = self.documents.get(p.text_document.uri.as_str())?;
        let pf = &doc.parsed;
        let symbols = document_symbols(&pf.ast, &pf.source).into_iter().map(|s| symbol(pf, s)).collect();
        Some(DocumentSymbolResponse::Nested(symbols))
    }

    pub(crate) fn folding_ranges(&self, p: FoldingRangeParams) -> Option<Vec<FoldingRange>> {
        let doc = self.documents.get(p.text_document.uri.as_str())?;
        let pf = &doc.parsed;
        Some(
            folding_ranges(&pf.ast, &pf.source, &pf.spans)
                .into_iter()
                .map(|r| FoldingRange {
                    start_line: r.start_line,
                    end_line: r.end_line,
                    kind: Some(match r.kind {
                        FoldKind::Comment => FoldingRangeKind::Comment,
                        FoldKind::Region => FoldingRangeKind::Region,
                    }),
                    ..FoldingRange::default()
                })
                .collect(),
        )
    }

    pub(crate) fn selection_ranges(&self, p: SelectionRangeParams) -> Option<Vec<SelectionRange>> {
        let doc = self.documents.get(p.text_document.uri.as_str())?;
        let pf = &doc.parsed;
        Some(
            p.positions
                .into_iter()
                .map(|pos| {
                    // Innermost first; LSP wants the innermost with parents.
                    let chain = selection_ranges(pf, crate::convert::line_col(pos));
                    let mut node: Option<SelectionRange> = None;
                    for span in chain.into_iter().rev() {
                        node = Some(SelectionRange { range: range(pf, span), parent: node.map(Box::new) });
                    }
                    node.unwrap_or(SelectionRange { range: Range { start: pos, end: pos }, parent: None })
                })
                .collect(),
        )
    }

    pub(crate) fn formatting(&self, p: DocumentFormattingParams) -> Option<Vec<TextEdit>> {
        if !self.settings.format.enabled {
            return None;
        }
        let doc = self.documents.get(p.text_document.uri.as_str())?;
        let pf = &doc.parsed;
        let opts = FormatOptions {
            tab_size: p.options.tab_size.max(1),
            insert_spaces: p.options.insert_spaces,
            print_width: self.settings.format.print_width,
            insert_final_newline: p.options.insert_final_newline.unwrap_or(true),
        };
        let text = format_file(pf, &opts)?;
        let whole = range(pf, ron_analyzer::spans::ByteSpan::new(0, pf.source.len() as u32));
        Some(vec![TextEdit { range: whole, new_text: text }])
    }
}

#[allow(deprecated)] // `DocumentSymbol::deprecated` must still be filled in.
fn symbol(pf: &ParsedFile, s: RonSymbol) -> DocumentSymbol {
    DocumentSymbol {
        name: if s.name.is_empty() { " ".into() } else { s.name },
        detail: (!s.detail.is_empty()).then_some(s.detail),
        kind: symbol_kind(s.kind),
        tags: None,
        deprecated: None,
        range: range(pf, s.range),
        selection_range: range(pf, s.selection_range),
        children: Some(s.children.into_iter().map(|c| symbol(pf, c)).collect()),
    }
}
