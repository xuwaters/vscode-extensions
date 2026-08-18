//! BibTeX: `.bib` documents, and the citations in `.typ` files that point into
//! them.
//!
//! A bibliography is part of a typst project, so it is part of the language
//! server. Two halves:
//!
//! * **In a `.bib` file** every feature routes here instead of to typst — the
//!   dispatch table is shared, so each handler asks [`Server::bib_of`] first and
//!   takes this path when the answer is `Some`. Running typst's parser over
//!   BibTeX would produce nonsense symbols, nonsense colours, and — with the
//!   formatter — a destroyed file.
//! * **In a `.typ` file** hover, goto-definition, and completion fall back to
//!   the bibliography when typst-ide has nothing to say about an `@key`. Upstream
//!   only knows the citation keys of the *last compiled document*, so before the
//!   first compile, or when `bibliography()` has not been written yet, this is
//!   the difference between working and not.
//!
//! The `.bib` text lives in the same [`Source`] overlay every other document
//! does — typst's parse of it is meaningless, but the line index and the
//! incremental `edit` are exactly what the LSP layer needs, and re-parsing the
//! bibliography per request costs microseconds on files this size.

use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionTextEdit, Diagnostic,
    DiagnosticRelatedInformation, DiagnosticSeverity, Documentation, DocumentLink,
    DocumentSymbol, FoldingRange, Hover, HoverContents, InsertTextFormat, Location,
    MarkupContent, MarkupKind, Position, PublishDiagnosticsParams, Range, SelectionRange,
    SemanticToken, SymbolKind, TextEdit, Uri,
};
use typst::syntax::{FileId, LinkedNode, Side, Source, SyntaxKind};

use crate::bib::{self, Bib, Entry, EntryKind, Severity, TokenKind};
use crate::convert::range_to_lsp;
use crate::features::semantic_tokens;
use crate::settings::SemanticTokensMode;
use crate::{Ports, Server};

/// The fields whose value is a citation key rather than prose.
const KEY_VALUED_FIELDS: &[&str] = &["crossref", "xdata", "related", "ids"];

/// Whether a file id names a BibTeX bibliography.
pub fn is_bib(id: FileId) -> bool {
    let path = id.get();
    let vpath = path.vpath().get_without_slash().to_ascii_lowercase();
    vpath.ends_with(".bib") || vpath.ends_with(".bibtex")
}

impl<Q: Ports> Server<Q> {
    /// The document behind a URI, parsed as BibTeX — `None` for anything that
    /// is not a `.bib` file, which is what every handler branches on.
    pub(crate) fn bib_of(&self, uri: &Uri) -> Option<(FileId, Source, Bib)> {
        let (id, source) = self.source_of(uri)?;
        if !is_bib(id) {
            return None;
        }
        let bib = Bib::parse(source.text());
        Some((id, source, bib))
    }

    /// Every bibliography the server can see: the ones the compile read, plus
    /// the ones the host reported in `typst/workspaceFiles`.
    pub(crate) fn bib_files(&self) -> Vec<FileId> {
        let mut ids: Vec<FileId> = typst_ide::IdeWorld::files(self.session().world())
            .into_iter()
            .filter(|id| is_bib(*id))
            .collect();
        for id in &self.workspace_files {
            if is_bib(*id) && !ids.contains(id) {
                ids.push(*id);
            }
        }
        ids
    }

    /// Find a citation key across every known bibliography.
    pub(crate) fn citation(&self, key: &str) -> Option<(FileId, Source, Entry)> {
        for id in self.bib_files() {
            let Ok(source) = typst::World::source(self.session().world(), id) else {
                continue;
            };
            let bib = Bib::parse(source.text());
            if let Some(entry) = bib.find(key) {
                let entry = entry.clone();
                return Some((id, source, entry));
            }
        }
        None
    }

    /// Every citation key the known bibliographies define, with a one-line
    /// summary, for completion.
    pub(crate) fn citation_keys(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for id in self.bib_files() {
            let Ok(source) = typst::World::source(self.session().world(), id) else {
                continue;
            };
            for entry in Bib::parse(source.text()).references() {
                let Some(key) = entry.key.clone() else { continue };
                if out.iter().any(|(existing, _): &(String, String)| *existing == key) {
                    continue;
                }
                out.push((key, entry.summary()));
            }
        }
        out
    }

    // ── Diagnostics ──────────────────────────────────────────────────────────

    /// Publish a bibliography's syntax errors and lint warnings.
    ///
    /// Runs straight off the edit rather than off the compile: parsing is
    /// instant, and a `.bib` file that is not referenced by any `bibliography()`
    /// call would otherwise never be checked at all.
    pub(crate) fn publish_bib_diagnostics(&mut self, id: FileId) {
        let Some(uri) = self.uris().to_uri(id) else { return };
        let key = uri.as_str().to_string();

        if !self.settings().diagnostics.enabled {
            if self.bib_published.remove(&key) {
                self.send_bib_diagnostics(uri, Vec::new());
            }
            return;
        }

        let Ok(source) = typst::World::source(self.session().world(), id) else { return };
        let bib = Bib::parse(source.text());

        let diagnostics: Vec<Diagnostic> = bib
            .problems
            .iter()
            .map(|problem| Diagnostic {
                range: range_to_lsp(&source, problem.range.clone()),
                severity: Some(match problem.severity {
                    Severity::Error => DiagnosticSeverity::ERROR,
                    Severity::Warning => DiagnosticSeverity::WARNING,
                }),
                source: Some("bibtex".into()),
                message: problem.message.clone(),
                related_information: problem.related.as_ref().map(|(range, message)| {
                    vec![DiagnosticRelatedInformation {
                        location: Location {
                            uri: uri.clone(),
                            range: range_to_lsp(&source, range.clone()),
                        },
                        message: message.clone(),
                    }]
                }),
                ..Diagnostic::default()
            })
            .collect();

        // A file that had problems and now has none must be published as an
        // empty array, or the squiggles stay put.
        if diagnostics.is_empty() && !self.bib_published.contains(&key) {
            return;
        }
        if diagnostics.is_empty() {
            self.bib_published.remove(&key);
        } else {
            self.bib_published.insert(key);
        }

        self.send_bib_diagnostics(uri, diagnostics);
    }

    /// Clear a closed bibliography's diagnostics.
    pub(crate) fn clear_bib_diagnostics(&mut self, id: FileId) {
        let Some(uri) = self.uris().to_uri(id) else { return };
        if self.bib_published.remove(uri.as_str()) {
            self.send_bib_diagnostics(uri, Vec::new());
        }
    }

    fn send_bib_diagnostics(&mut self, uri: Uri, diagnostics: Vec<Diagnostic>) {
        let params = PublishDiagnosticsParams { uri, diagnostics, version: None };
        if let Ok(params) = serde_json::to_value(params) {
            self.notify("textDocument/publishDiagnostics", params);
        }
    }

    // ── Features, one per LSP method ─────────────────────────────────────────

    /// `textDocument/documentSymbol` for a bibliography: one symbol per entry,
    /// its fields nested underneath.
    pub(crate) fn bib_symbols(&self, source: &Source, bib: &Bib) -> Vec<DocumentSymbol> {
        bib.entries.iter().map(|entry| symbol_of(source, entry)).collect()
    }

    /// `textDocument/foldingRange`: one region per entry.
    pub(crate) fn bib_folding(&self, source: &Source, bib: &Bib) -> Vec<FoldingRange> {
        let lines = source.lines();
        bib.entries
            .iter()
            .filter_map(|entry| {
                let start = lines.byte_to_line(entry.range.start)?;
                let end = lines.byte_to_line(entry.range.end.saturating_sub(1))?;
                (end > start).then_some(FoldingRange {
                    start_line: start as u32,
                    end_line: end as u32,
                    ..FoldingRange::default()
                })
            })
            .collect()
    }

    /// `textDocument/semanticTokens/*`: colour from the BibTeX parse.
    pub(crate) fn bib_tokens(&self, source: &Source, bib: &Bib) -> Vec<SemanticToken> {
        if self.settings().semantic_tokens != SemanticTokensMode::Enable {
            return Vec::new();
        }

        let mut absolute = Vec::new();
        for token in &bib.tokens {
            let index = semantic_tokens::index_of(token_type(token.kind));
            semantic_tokens::push_split_by_line(
                token.range.clone(),
                index,
                source,
                &mut absolute,
            );
        }
        semantic_tokens::encode(&absolute)
    }

    /// `textDocument/hover`: the entry under the cursor, or what a field means.
    pub(crate) fn bib_hover(
        &self,
        source: &Source,
        bib: &Bib,
        cursor: usize,
    ) -> Option<Hover> {
        let entry = bib.entry_at(cursor)?;

        let (value, range) = if entry.type_range.contains(&cursor) {
            let description = bib::ENTRY_TYPES
                .iter()
                .find(|(name, _)| *name == entry.type_name)
                .map(|(_, description)| *description)
                .unwrap_or("An entry type typst does not know");
            (
                format!("`@{}` — {description}", entry.type_name),
                entry.type_range.clone(),
            )
        } else if entry.key_range.contains(&cursor) {
            (entry.markdown(), entry.key_range.clone())
        } else if let Some(field) = entry.fields.iter().find(|field| {
            field.name_range.contains(&cursor)
        }) {
            let description = bib::FIELDS
                .iter()
                .find(|(name, _)| *name == field.name)
                .map(|(_, description)| *description)
                .unwrap_or("Not a field typst reads");
            let mut text = format!("`{}` — {description}", field.name);
            if let Some(value) = field.value.as_ref() {
                text.push_str(&format!("\n\n{}", value.text));
            }
            (text, field.name_range.clone())
        } else {
            // Inside a value: a `crossref` names another entry, and a bare word
            // names a `@string` abbreviation. Both are worth resolving.
            let field = entry
                .fields
                .iter()
                .find(|field| field.value.as_ref().is_some_and(|v| v.range.contains(&cursor)))?;
            let value = field.value.as_ref()?;

            if KEY_VALUED_FIELDS.contains(&field.name.as_str()) {
                let target = bib.find(value.text.trim())?;
                (target.markdown(), value.range.clone())
            } else {
                let word = word_at(source.text(), cursor)?;
                let (_, definition) = bib.abbreviation(&word.1)?;
                let text = definition.value.as_ref().map(|v| v.text.as_str()).unwrap_or("");
                (format!("`@string` `{}` — {text}", word.1), word.0)
            }
        };

        Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value,
            }),
            range: Some(range_to_lsp(source, range)),
        })
    }

    /// `textDocument/definition`: `crossref` and `@string` references.
    pub(crate) fn bib_definition(
        &self,
        id: FileId,
        source: &Source,
        bib: &Bib,
        cursor: usize,
    ) -> Option<Location> {
        let entry = bib.entry_at(cursor)?;
        let field = entry
            .fields
            .iter()
            .find(|field| field.value.as_ref().is_some_and(|v| v.range.contains(&cursor)))?;
        let value = field.value.as_ref()?;

        if KEY_VALUED_FIELDS.contains(&field.name.as_str()) {
            let key = value.text.trim();
            // A `crossref` may point into another file in a split bibliography.
            if let Some(target) = bib.find(key) {
                return Some(Location {
                    uri: self.uris().to_uri(id)?,
                    range: range_to_lsp(source, target.key_range.clone()),
                });
            }
            let (file, target_source, target) = self.citation(key)?;
            return Some(Location {
                uri: self.uris().to_uri(file)?,
                range: range_to_lsp(&target_source, target.key_range.clone()),
            });
        }

        let (_, name) = word_at(source.text(), cursor)?;
        let (_, definition) = bib.abbreviation(&name)?;
        Some(Location {
            uri: self.uris().to_uri(id)?,
            range: range_to_lsp(source, definition.name_range.clone()),
        })
    }

    /// `textDocument/documentLink`: `url` and `doi` fields.
    pub(crate) fn bib_links(&self, source: &Source, bib: &Bib) -> Vec<DocumentLink> {
        let mut out = Vec::new();

        for entry in bib.references() {
            for field in &entry.fields {
                let Some(value) = field.value.as_ref() else { continue };
                let target = match field.name.as_str() {
                    "url" | "howpublished" => value.text.trim().to_string(),
                    // A DOI is written bare; the resolver is what makes it
                    // clickable.
                    "doi" => format!("https://doi.org/{}", value.text.trim()),
                    _ => continue,
                };

                let scheme = target.split(':').next().unwrap_or_default();
                if !super::links::ALLOWED_SCHEMES.contains(&scheme) {
                    continue;
                }
                let Ok(target) = target.parse::<Uri>() else { continue };

                out.push(DocumentLink {
                    range: range_to_lsp(source, inner_range(source.text(), value)),
                    target: Some(target),
                    tooltip: None,
                    data: None,
                });
            }
        }

        out
    }

    /// `textDocument/formatting`: the canonical layout.
    pub(crate) fn bib_formatting(&self, source: &Source) -> Option<Vec<TextEdit>> {
        let indent = self.settings().formatter.indent_size;
        let formatted = bib::format(source.text(), indent)?;
        if formatted == source.text() {
            return Some(Vec::new());
        }

        let lines = source.lines();
        let last = lines.len_lines().saturating_sub(1);
        let end_column = source
            .text()
            .get(lines.line_to_range(last).unwrap_or_default())
            .unwrap_or_default()
            .trim_end_matches(['\n', '\r'])
            .chars()
            .map(char::len_utf16)
            .sum::<usize>();

        Some(vec![TextEdit {
            range: Range {
                start: Position { line: 0, character: 0 },
                end: Position { line: last as u32, character: end_column as u32 },
            },
            new_text: formatted,
        }])
    }

    /// `textDocument/selectionRange`: value → field → entry → file.
    pub(crate) fn bib_selection_range(
        &self,
        source: &Source,
        bib: &Bib,
        cursor: usize,
    ) -> SelectionRange {
        let whole = SelectionRange {
            range: range_to_lsp(source, 0..source.text().len()),
            parent: None,
        };

        let Some(entry) = bib.entry_at(cursor) else { return whole };
        let mut current = SelectionRange {
            range: range_to_lsp(source, entry.range.clone()),
            parent: Some(Box::new(whole)),
        };

        if let Some(field) = entry.fields.iter().find(|field| {
            let end = field.value.as_ref().map_or(field.name_range.end, |v| v.range.end);
            (field.name_range.start..end).contains(&cursor)
        }) {
            let end = field.value.as_ref().map_or(field.name_range.end, |v| v.range.end);
            current = SelectionRange {
                range: range_to_lsp(source, field.name_range.start..end),
                parent: Some(Box::new(current)),
            };

            if let Some(value) = field.value.as_ref()
                && value.range.contains(&cursor)
            {
                current = SelectionRange {
                    range: range_to_lsp(source, inner_range(source.text(), value)),
                    parent: Some(Box::new(current)),
                };
            }
        }

        current
    }

    /// `textDocument/completion`: entry types, field names, and citation keys.
    pub(crate) fn bib_completion(
        &self,
        source: &Source,
        bib: &Bib,
        cursor: usize,
    ) -> Vec<CompletionItem> {
        let text = source.text();

        let Some(entry) = bib.entry_at(cursor) else {
            return self.entry_type_items(source, cursor);
        };

        // Inside a value, the useful completions are the ones that name
        // something: another entry, or an abbreviation.
        if let Some(field) = entry
            .fields
            .iter()
            .find(|field| field.value.as_ref().is_some_and(|v| v.range.contains(&cursor)))
        {
            let (range, prefix) =
                word_at(text, cursor).unwrap_or_else(|| (cursor..cursor, String::new()));
            let range = range_to_lsp(source, range);

            if KEY_VALUED_FIELDS.contains(&field.name.as_str()) {
                return bib
                    .references()
                    .filter(|target| target.key.as_deref() != entry.key.as_deref())
                    .filter_map(|target| {
                        let key = target.key.clone()?;
                        Some(item(
                            key.clone(),
                            CompletionItemKind::REFERENCE,
                            Some(target.summary()),
                            None,
                            range,
                            key,
                        ))
                    })
                    .collect();
            }

            // A quoted or braced value is prose, not an abbreviation.
            if !prefix.is_empty() && is_bare_word_value(text, field, cursor) {
                return bib
                    .entries
                    .iter()
                    .filter(|entry| entry.kind == EntryKind::String)
                    .flat_map(|entry| entry.fields.iter())
                    .map(|definition| {
                        let detail =
                            definition.value.as_ref().map(|value| value.text.clone());
                        item(
                            definition.name.clone(),
                            CompletionItemKind::CONSTANT,
                            detail,
                            None,
                            range,
                            definition.name.clone(),
                        )
                    })
                    .collect();
            }

            return Vec::new();
        }

        // The type name itself is still being typed: `@art|`.
        if entry.type_range.contains(&cursor) || entry.type_range.end == cursor {
            return self.entry_type_items(source, cursor);
        }

        // The citation key is the author's to invent; there is nothing to offer.
        if entry.key_range.contains(&cursor) || entry.key_range.end == cursor {
            return Vec::new();
        }

        self.field_items(source, entry, cursor)
    }

    /// `@article{…}` skeletons, at the top level of the file.
    fn entry_type_items(&self, source: &Source, cursor: usize) -> Vec<CompletionItem> {
        let text = source.text();
        // The replacement covers the `@` too, so `@art` does not become `@@article`.
        let (range, _) = word_at(text, cursor).unwrap_or((cursor..cursor, String::new()));
        let start = if text[..range.start].ends_with('@') {
            range.start - 1
        } else {
            range.start
        };
        let range = range_to_lsp(source, start..range.end);

        bib::ENTRY_TYPES
            .iter()
            .map(|(name, description)| {
                item(
                    format!("@{name}"),
                    CompletionItemKind::CLASS,
                    Some(description.to_string()),
                    Some(description.to_string()),
                    range,
                    entry_skeleton(name, self.settings().formatter.indent_size),
                )
            })
            .collect()
    }

    /// Field names an entry does not have yet.
    fn field_items(
        &self,
        source: &Source,
        entry: &Entry,
        cursor: usize,
    ) -> Vec<CompletionItem> {
        let text = source.text();
        let (range, _) =
            word_at(text, cursor).unwrap_or_else(|| (cursor..cursor, String::new()));

        // If an `=` already follows, the user is renaming a field rather than
        // adding one, and inserting a second `= {}` would be wrong.
        let has_assignment = text[range.end..].trim_start().starts_with('=');
        let lsp_range = range_to_lsp(source, range.clone());

        let present: Vec<&str> = entry
            .fields
            .iter()
            .filter(|field| field.name_range != range)
            .map(|field| field.name.as_str())
            .collect();

        let required: Vec<&str> = bib::required_fields(&entry.type_name)
            .iter()
            .filter_map(|alternatives| alternatives.first().copied())
            .collect();

        bib::FIELDS
            .iter()
            .filter(|(name, _)| !present.contains(name))
            .map(|(name, description)| {
                let new_text = if has_assignment {
                    name.to_string()
                } else {
                    format!("{name} = {{$1}},")
                };
                let mut completion = item(
                    name.to_string(),
                    CompletionItemKind::FIELD,
                    Some(description.to_string()),
                    Some(description.to_string()),
                    lsp_range,
                    new_text,
                );
                // A required field is what the reader is most likely reaching
                // for, so it sorts above the rest.
                completion.sort_text = Some(match required.contains(name) {
                    true => format!("0{name}"),
                    false => format!("1{name}"),
                });
                completion
            })
            .collect()
    }

    // ── The `.typ` side ──────────────────────────────────────────────────────

    /// The bibliography entry a `@key` or `<key>` in a typst file names.
    pub(crate) fn cited_entry(
        &self,
        source: &Source,
        cursor: usize,
    ) -> Option<(FileId, Source, Entry)> {
        let key = reference_name(source, cursor)?;
        self.citation(&key)
    }

    /// Citation keys to add to a typst file's completions.
    ///
    /// Only the ones upstream did not already offer: it knows the keys of the
    /// last compiled document, which is the better answer when it exists.
    pub(crate) fn citation_items(
        &self,
        source: &Source,
        cursor: usize,
        offered: &[CompletionItem],
    ) -> Vec<CompletionItem> {
        let Some((range, prefix)) = citation_prefix(source.text(), cursor) else {
            return Vec::new();
        };
        let lsp_range = range_to_lsp(source, range);

        self.citation_keys()
            .into_iter()
            .filter(|(key, _)| key.starts_with(&prefix) || prefix.is_empty())
            .filter(|(key, _)| {
                !offered.iter().any(|item| {
                    item.label == *key || item.label == format!("@{key}")
                })
            })
            .map(|(key, summary)| {
                let mut completion = item(
                    format!("@{key}"),
                    CompletionItemKind::REFERENCE,
                    Some(summary),
                    None,
                    lsp_range,
                    format!("@{key}"),
                );
                // After everything typst offered, which is ordered by relevance.
                completion.sort_text = Some(format!("z{key}"));
                completion
            })
            .collect()
    }
}

/// One entry as a nested symbol, its fields underneath.
fn symbol_of(source: &Source, entry: &Entry) -> DocumentSymbol {
    let children: Vec<DocumentSymbol> = entry
        .fields
        .iter()
        .map(|field| {
            let end = field.value.as_ref().map_or(field.name_range.end, |v| v.range.end);
            #[allow(deprecated)]
            DocumentSymbol {
                name: field.name.clone(),
                detail: field.value.as_ref().map(|value| value.text.clone()),
                kind: SymbolKind::FIELD,
                tags: None,
                deprecated: None,
                range: range_to_lsp(source, field.name_range.start..end),
                selection_range: range_to_lsp(source, field.name_range.clone()),
                children: None,
            }
        })
        .collect();

    let name = match (&entry.key, entry.kind) {
        (Some(key), _) => key.clone(),
        (None, _) => format!("@{}", entry.type_name),
    };

    #[allow(deprecated)]
    DocumentSymbol {
        name,
        detail: Some(entry.summary()),
        kind: match entry.kind {
            EntryKind::Reference => SymbolKind::CONSTANT,
            EntryKind::String => SymbolKind::VARIABLE,
            EntryKind::Preamble | EntryKind::Comment => SymbolKind::NAMESPACE,
        },
        tags: None,
        deprecated: None,
        range: range_to_lsp(source, entry.range.clone()),
        // The key, so picking the symbol reveals the line rather than the block.
        selection_range: range_to_lsp(
            source,
            match entry.key.is_some() {
                true => entry.key_range.clone(),
                false => entry.type_range.clone(),
            },
        ),
        children: (!children.is_empty()).then_some(children),
    }
}

/// A ready-to-fill entry, with the type's required fields as tab stops.
fn entry_skeleton(type_name: &str, indent: usize) -> String {
    let pad = " ".repeat(indent);
    let mut out = format!("@{type_name}{{${{1:key}},\n");

    let mut stop = 2;
    for alternatives in bib::required_fields(type_name) {
        let Some(name) = alternatives.first() else { continue };
        out.push_str(&format!("{pad}{name} = {{${stop}}},\n"));
        stop += 1;
    }
    if stop == 2 {
        // A type with no required fields still needs somewhere to type.
        out.push_str(&format!("{pad}title = {{$2}},\n"));
    }

    out.push('}');
    out
}

fn item(
    label: String,
    kind: CompletionItemKind,
    detail: Option<String>,
    documentation: Option<String>,
    range: Range,
    new_text: String,
) -> CompletionItem {
    let snippet = new_text.contains('$');
    CompletionItem {
        label,
        kind: Some(kind),
        detail,
        documentation: documentation.map(|value| {
            Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value,
            })
        }),
        text_edit: Some(CompletionTextEdit::Edit(TextEdit { range, new_text })),
        insert_text_format: Some(match snippet {
            true => InsertTextFormat::SNIPPET,
            false => InsertTextFormat::PLAIN_TEXT,
        }),
        ..CompletionItem::default()
    }
}

/// The identifier-shaped run of text around an offset, and its range.
fn word_at(text: &str, cursor: usize) -> Option<(std::ops::Range<usize>, String)> {
    let is_word = |character: char| {
        character.is_alphanumeric() || matches!(character, '-' | '_' | '.' | '+' | ':')
    };

    let cursor = cursor.min(text.len());
    let start = text[..cursor]
        .char_indices()
        .rev()
        .take_while(|(_, character)| is_word(*character))
        .last()
        .map(|(offset, _)| offset)
        .unwrap_or(cursor);
    let end = cursor
        + text[cursor..]
            .char_indices()
            .take_while(|(_, character)| is_word(*character))
            .map(|(offset, character)| offset + character.len_utf8())
            .last()
            .unwrap_or(0);

    (start < end).then(|| (start..end, text[start..end].to_string()))
}

/// Whether the cursor sits in a value written as a bare word — the only place
/// an `@string` abbreviation can go.
fn is_bare_word_value(text: &str, field: &bib::Field, cursor: usize) -> bool {
    let Some(value) = field.value.as_ref() else { return false };
    let Some(word) = word_at(text, cursor) else { return false };
    // Not inside a `{…}` or `"…"` run: those start at the value's first byte.
    !text[value.range.start..word.0.start].contains(['{', '"'])
}

/// A value's range without its delimiters.
fn inner_range(text: &str, value: &bib::Value) -> std::ops::Range<usize> {
    let raw = &text[value.range.clone()];
    match raw.chars().next() {
        Some('{' | '"') if raw.len() >= 2 => value.range.start + 1..value.range.end - 1,
        _ => value.range.clone(),
    }
}

/// `@knuth1984` or `<knuth1984>` at a cursor in a typst file, without its
/// delimiters.
pub(crate) fn reference_name(source: &Source, cursor: usize) -> Option<String> {
    let root = LinkedNode::new(source.root());
    let leaf = root
        .leaf_at(cursor, Side::Before)
        .or_else(|| root.leaf_at(cursor, Side::After))?;

    let mut node = Some(leaf);
    while let Some(current) = node {
        if matches!(current.kind(), SyntaxKind::Ref | SyntaxKind::Label) {
            let text = source.text().get(current.range())?;
            return Some(
                text.trim_start_matches(['@', '<']).trim_end_matches('>').to_string(),
            );
        }
        node = current.parent().cloned();
    }
    None
}

/// The `@name` being typed at a cursor, as a range and the name so far.
///
/// Read off the text rather than the tree: a lone `@` is not a `Ref` node yet,
/// and that is exactly the moment the completion list is asked for.
fn citation_prefix(text: &str, cursor: usize) -> Option<(std::ops::Range<usize>, String)> {
    let cursor = cursor.min(text.len());
    let before = &text[..cursor];

    let start = before
        .char_indices()
        .rev()
        .take_while(|(_, character)| {
            character.is_alphanumeric() || matches!(character, '-' | '_' | '.' | ':')
        })
        .last()
        .map(|(offset, _)| offset)
        .unwrap_or(cursor);

    if !text[..start].ends_with('@') {
        return None;
    }
    Some((start - 1..cursor, text[start..cursor].to_string()))
}

/// Bib token → semantic token type name, in the legend's vocabulary.
fn token_type(kind: TokenKind) -> &'static str {
    match kind {
        TokenKind::EntryType => "keyword",
        TokenKind::Key => "label",
        TokenKind::FieldName => "property",
        TokenKind::Value => "string",
        TokenKind::Number => "number",
        TokenKind::Macro => "variable",
        TokenKind::Punct => "punct",
        TokenKind::Comment => "comment",
    }
}

#[cfg(test)]
mod tests {
    use typst::syntax::{RootedPath, VirtualPath, VirtualRoot};

    use super::*;

    fn project_file(path: &str) -> FileId {
        FileId::new(RootedPath::new(
            VirtualRoot::Project,
            VirtualPath::new(path).expect("valid virtual path"),
        ))
    }

    #[test]
    fn a_bib_path_is_recognised_whatever_its_case() {
        assert!(is_bib(project_file("refs.bib")));
        assert!(is_bib(project_file("chapters/Refs.BIB")));
        assert!(!is_bib(project_file("main.typ")));
        assert!(!is_bib(project_file("bib")));
    }

    #[test]
    fn a_word_is_found_from_either_end_of_itself() {
        let text = "author = {x}";
        assert_eq!(word_at(text, 0).unwrap().1, "author");
        assert_eq!(word_at(text, 3).unwrap().1, "author");
        assert_eq!(word_at(text, 6).unwrap().1, "author");
        assert!(word_at(text, 7).is_none(), "a space has no word");
    }

    #[test]
    fn a_citation_prefix_needs_an_at_sign() {
        assert_eq!(
            citation_prefix("see @knuth", 10).unwrap(),
            (4..10, "knuth".to_string())
        );
        assert_eq!(citation_prefix("see @", 5).unwrap(), (4..5, String::new()));
        assert!(citation_prefix("see knuth", 9).is_none());
    }

    #[test]
    fn a_skeleton_carries_the_required_fields_as_tab_stops() {
        let skeleton = entry_skeleton("article", 2);
        assert!(skeleton.starts_with("@article{${1:key},\n"), "{skeleton}");
        assert!(skeleton.contains("author = {$2},"), "{skeleton}");
        assert!(skeleton.contains("journal = {$4},"), "{skeleton}");
        assert!(skeleton.ends_with("}"), "{skeleton}");
    }
}
