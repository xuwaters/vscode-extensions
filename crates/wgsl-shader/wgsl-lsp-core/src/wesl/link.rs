//! Linking: a WESL file and everything it imports, as one WGSL module naga
//! can type-check.
//!
//! The file being checked is the root. It goes in whole, minus its imports;
//! from every other module goes only what is reachable from the root — each
//! declaration a name in already-linked code resolves to, and what *that*
//! names in turn. A declaration from another module is renamed so it cannot
//! clash, and every use of it — an imported name, an alias, a qualified path
//! like `lights::point::shade` — is rewritten to the new name.
//!
//! ```text
//! main.wesl                               linked
//! import package::lighting::attenuate;
//! fn f() -> f32 {                         fn f() -> f32 {
//!     return attenuate(2.0);                  return package__lighting__attenuate(2.0);
//! }                                       }
//!                                         fn package__lighting__attenuate(d: f32) -> f32 { … }
//! ```
//!
//! Conditional translation ([`super::conditions`]) runs first, file by file,
//! under the `wesl.features` setting.
//!
//! Every byte of the linked text remembers where it came from, so a problem
//! naga finds maps back to a span in the root — or, when it is inside another
//! module, to the use in the root that pulled that code in. Renamed
//! declarations are named by their WESL paths again in naga's messages.
//!
//! **When nothing is linked.** A path that names nothing, a dependency the
//! server cannot see, a condition [`super::conditions`] cannot read: each
//! leaves a module naga would misjudge, so [`Server::wesl_link`] gives up
//! rather than report errors that are not there.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};

use analyzer_core::spans::{ByteSpan, SpanTable};
use lsp_types::{Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, Location, Uri};
use wgsl_syntax::lexer::TokenKind;
use wgsl_syntax::{Language, Parsed};

use super::conditions::{self, Translation};
use super::{
    FullPath, Package, Resolution, follows_path_separator, in_import_statement, is_within,
    path_at, precedes_path_separator,
};
use crate::Server;
use crate::state::Document;

/// The `source` field of a problem naga found in linked code.
const NAGA: &str = "naga";

/// Names that are only words in a template list — `var<storage, read_write>`,
/// `texture_storage_2d<rgba8unorm, write>` — and must not be rewritten there
/// even when a module declares something of the same name.
const ENUMERANTS: &[&str] = &[
    "function", "private", "workgroup", "uniform", "storage", "handle", "read", "write",
    "read_write", "rgba8unorm", "rgba8snorm", "rgba8uint", "rgba8sint", "rgba16uint",
    "rgba16sint", "rgba16float", "r32uint", "r32sint", "r32float", "rg32uint", "rg32sint",
    "rg32float", "rgba32uint", "rgba32sint", "rgba32float", "bgra8unorm",
];

/// Attributes whose arguments are words of their own, not names in scope:
/// `@builtin(position)` means the builtin whatever `position` is declared as.
const WORD_ATTRIBUTES: &[&str] = &["@builtin", "@interpolate", "@diagnostic"];

/// A WESL file linked with what it imports.
#[derive(Debug)]
pub struct Linked {
    /// The WGSL naga reads.
    pub source: String,
    /// Where each run of `source` came from, in order.
    pieces: Vec<Piece>,
    chunks: Vec<Chunk>,
    /// Every file that contributed, the root first.
    files: Vec<LinkedFile>,
    /// Each renamed declaration's new name and its WESL path.
    names: Vec<(String, String)>,
}

#[derive(Debug, Clone, Copy)]
struct Piece {
    /// Where the run starts in the linked text.
    linked: u32,
    len: u32,
    chunk: usize,
    /// The original text: the same bytes when `copied`, otherwise the path or
    /// name the run was rewritten from.
    original: ByteSpan,
    copied: bool,
}

/// A run of one file's text, copied into the linked module.
#[derive(Debug, Clone)]
struct Chunk {
    file: usize,
    span: ByteSpan,
    edits: Vec<Edit>,
    /// Where in the root this code was first needed from; `None` for the
    /// root's own chunk.
    via: Option<ByteSpan>,
}

#[derive(Debug, Clone)]
struct LinkedFile {
    uri: Uri,
    text: String,
    /// The module's WESL path, as messages name it.
    module: String,
}

/// A change to a chunk's text: `span` becomes `replacement`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Edit {
    span: ByteSpan,
    replacement: String,
}

/// What a name in linked code stands for, before it is resolved.
enum Target {
    /// A module-scope declaration of the same file.
    Local(String),
    /// A path, or a name an import brought in.
    Path(FullPath),
}

impl Server {
    /// Link a WESL document with everything it imports, under the configured
    /// features. `None` when the result would not be a module worth checking
    /// — see the module docs.
    pub fn wesl_link(&self, document: &Document) -> Option<Linked> {
        let mut linker = Linker {
            server: self,
            features: &self.settings().wesl.features,
            package: self.package_of(document.uri.as_str()),
            units: Vec::new(),
            by_uri: HashMap::new(),
            prefixes: HashSet::new(),
            included: HashSet::new(),
            chunks: Vec::new(),
            names: Vec::new(),
        };
        linker.add_unit(
            document.uri.clone(),
            Cow::Borrowed(document.text()),
            Cow::Borrowed(document.parsed()),
        )?;
        linker.link()
    }

    /// Everything naga finds wrong with a WESL document once it is linked
    /// with its imports. Nothing when it could not be linked.
    pub fn wesl_link_diagnostics(&self, document: &Document) -> Vec<Diagnostic> {
        let Some(linked) = self.wesl_link(document) else {
            return Vec::new();
        };
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        for problem in crate::analysis::check_linked(&linked.source) {
            let message = linked.demangle(&problem.message);
            let diagnostic = match linked.locate(problem.span) {
                Some((chunk, span)) => match linked.chunks[chunk].via {
                    None => Diagnostic {
                        range: document.range(span),
                        severity: Some(DiagnosticSeverity::ERROR),
                        source: Some(NAGA.to_string()),
                        message,
                        ..Diagnostic::default()
                    },
                    // Inside another module: on the use that brought it in,
                    // pointing at where it actually is.
                    Some(via) => {
                        let file = &linked.files[linked.chunks[chunk].file];
                        let lines = SpanTable::new(&file.text);
                        let location = Location {
                            uri: file.uri.clone(),
                            range: crate::convert::span_to_range(&file.text, &lines, span),
                        };
                        Diagnostic {
                            range: document.range(via),
                            severity: Some(DiagnosticSeverity::ERROR),
                            source: Some(NAGA.to_string()),
                            message: format!("in `{}`: {message}", file.module),
                            related_information: Some(vec![DiagnosticRelatedInformation {
                                location,
                                message: "the problem is here".to_string(),
                            }]),
                            ..Diagnostic::default()
                        }
                    }
                },
                None => Diagnostic {
                    range: document.range(ByteSpan::EMPTY),
                    severity: Some(DiagnosticSeverity::ERROR),
                    source: Some(NAGA.to_string()),
                    message,
                    ..Diagnostic::default()
                },
            };
            // naga gives one message several spans; inside another module
            // they all land on the same use, so they become one diagnostic
            // pointing at each.
            match diagnostics
                .iter_mut()
                .find(|d| d.range == diagnostic.range && d.message == diagnostic.message)
            {
                Some(existing) => {
                    if let Some(more) = diagnostic.related_information {
                        existing.related_information.get_or_insert_with(Vec::new).extend(more);
                    }
                }
                None => diagnostics.push(diagnostic),
            }
        }
        diagnostics
    }
}

impl Linked {
    /// The chunk and the original span a span of the linked text came from.
    fn locate(&self, span: ByteSpan) -> Option<(usize, ByteSpan)> {
        let piece_at = |offset: u32| {
            let index = self.pieces.partition_point(|piece| piece.linked <= offset);
            self.pieces.get(index.checked_sub(1)?)
        };
        let original = |piece: &Piece, offset: u32, end: bool| {
            if piece.copied {
                piece.original.start + (offset - piece.linked).min(piece.len)
            } else if end {
                piece.original.end
            } else {
                piece.original.start
            }
        };

        let first = piece_at(span.start)?;
        let start = original(first, span.start, false);
        let last_byte = span.end.saturating_sub(1).max(span.start);
        let end = match piece_at(last_byte) {
            Some(last) if last.chunk == first.chunk => {
                if last.copied { original(last, span.end, true) } else { last.original.end }
            }
            _ => first.original.end,
        };
        Some((first.chunk, ByteSpan::new(start, end.max(start))))
    }

    /// `message` with every renamed declaration called by its WESL path.
    fn demangle(&self, message: &str) -> String {
        let mut names: Vec<&(String, String)> = self.names.iter().collect();
        // A longer name first, so one that extends another is not cut short.
        names.sort_by_key(|(mangled, _)| std::cmp::Reverse(mangled.len()));
        let mut message = message.to_string();
        for (mangled, path) in names {
            message = message.replace(mangled.as_str(), path);
        }
        message
    }
}

/// One file taking part in a link.
struct Unit<'a> {
    uri: Uri,
    text: Cow<'a, str>,
    parsed: Cow<'a, Parsed>,
    translation: Translation,
    /// What a declaration's new name starts with: `package__lights__point`.
    prefix: String,
    /// The module's WESL path: `package::lights::point`.
    module: String,
}

struct Linker<'a> {
    server: &'a Server,
    features: &'a BTreeMap<String, bool>,
    /// The root's package, which names every module's path.
    package: Package,
    units: Vec<Unit<'a>>,
    by_uri: HashMap<String, usize>,
    prefixes: HashSet<String>,
    /// `(unit, name)` for each declaration already queued.
    included: HashSet<(usize, String)>,
    chunks: Vec<Chunk>,
    names: Vec<(String, String)>,
}

impl<'a> Linker<'a> {
    fn link(mut self) -> Option<Linked> {
        // The root: its whole text, minus its imports.
        let root = &self.units[0];
        let whole = ByteSpan::new(0, root.text.len() as u32);
        let mut removed_imports: Vec<ByteSpan> = Vec::new();
        for import in &root.parsed.imports {
            let span = root.parsed.symbols[import.symbol].full_span;
            if !removed_imports.contains(&span) {
                removed_imports.push(span);
            }
        }
        // In place before its edits are worked out, which queues what it
        // pulls in behind it.
        self.chunks.push(Chunk { file: 0, span: whole, edits: Vec::new(), via: None });
        let mut edits = self.edits(0, whole, None)?;
        let cut = |span| Edit { span, replacement: String::new() };
        edits.extend(removed_imports.into_iter().map(cut));
        self.chunks[0].edits = edits;

        // Everything that pulls in, until nothing new does. Chunks are pushed
        // as they are found, so the loop index runs to a moving end.
        let mut next = 1;
        while next < self.chunks.len() {
            let Chunk { file, span, via, .. } = self.chunks[next].clone();
            let edits = self.edits(file, span, via)?;
            self.chunks[next].edits = edits;
            next += 1;
        }

        let directives = self.directives();
        Some(self.write(directives))
    }

    /// Take part a file — `None` when its conditions cannot be read.
    fn add_unit(&mut self, uri: Uri, text: Cow<'a, str>, parsed: Cow<'a, Parsed>) -> Option<usize> {
        let translation = conditions::translate(&parsed, &text, self.features).ok()?;
        let (prefix, module) = self.module_names(uri.as_str());
        let index = self.units.len();
        self.by_uri.insert(uri.as_str().to_string(), index);
        self.units.push(Unit { uri, text, parsed, translation, prefix, module });
        Some(index)
    }

    /// The unit for the file at `uri`, read from the open document or the
    /// index.
    fn unit(&mut self, uri: &Uri) -> Option<usize> {
        if let Some(&index) = self.by_uri.get(uri.as_str()) {
            return Some(index);
        }
        let server = self.server;
        if let Some(document) = server.document(uri) {
            return self.add_unit(
                uri.clone(),
                Cow::Borrowed(document.text()),
                Cow::Borrowed(document.parsed()),
            );
        }
        let text = server.index().source(uri.as_str())?;
        let language = match uri.as_str().rsplit_once('.') {
            Some((_, "wesl")) => Language::Wesl,
            _ => Language::Wgsl,
        };
        let parsed = wgsl_syntax::parse(text, language);
        self.add_unit(uri.clone(), Cow::Borrowed(text), Cow::Owned(parsed))
    }

    /// A module's name prefix and WESL path, from where its file sits
    /// relative to the root's package and dependencies.
    fn module_names(&mut self, uri: &str) -> (String, String) {
        let stem = uri.rsplit_once('.').map_or(uri, |(stem, _)| stem);
        let relative = |dir: &str| -> Option<Vec<String>> {
            let rest = stem.strip_prefix(dir)?;
            is_within(stem, dir).then(|| {
                rest.split('/').filter(|part| !part.is_empty()).map(String::from).collect()
            })
        };
        let path: Option<Vec<String>> = relative(&self.package.root)
            .map(|rest| [vec!["package".to_string()], rest].concat())
            .or_else(|| {
                self.package.dependencies.iter().find_map(|(name, dir)| {
                    relative(dir).map(|rest| [vec![name.clone()], rest].concat())
                })
            });
        let path = path.unwrap_or_else(|| vec![super::file_name(stem).to_string()]);
        let module = path.join("::");

        // An identifier, and one no other module already starts with.
        let base: String = path
            .iter()
            .map(|part| {
                part.chars()
                    .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("__");
        let base =
            if base.starts_with(|c: char| c.is_alphabetic()) { base } else { format!("m_{base}") };
        let mut prefix = base.clone();
        let mut n = 1;
        while !self.prefixes.insert(prefix.clone()) {
            n += 1;
            prefix = format!("{base}_{n}");
        }
        (prefix, module)
    }

    /// The edits linking makes to `span` of unit `u`, queueing every
    /// declaration of another module it names. `via` is where in the root
    /// this code was needed from, which code it pulls in inherits.
    fn edits(&mut self, u: usize, span: ByteSpan, via: Option<ByteSpan>) -> Option<Vec<Edit>> {
        let mut edits: Vec<Edit> = Vec::new();
        let mut targets: Vec<(ByteSpan, Target)> = Vec::new();
        {
            let unit = &self.units[u];
            let (parsed, text) = (&*unit.parsed, &*unit.text);
            let within = |inner: ByteSpan| span.start <= inner.start && inner.end <= span.end;
            for &removed in unit.translation.removed.iter().filter(|s| within(**s)) {
                edits.push(Edit { span: removed, replacement: String::new() });
            }
            for &blanked in unit.translation.blanked.iter().filter(|s| within(**s)) {
                edits.push(Edit { span: blanked, replacement: String::new() });
            }

            let words = word_arguments(parsed, text);
            for reference in parsed.references.iter().filter(|r| within(r.span)) {
                let start = reference.span.start;
                if unit.translation.is_removed(reference.span)
                    || in_import_statement(parsed, start)
                    || words.iter().any(|w| w.contains(start))
                    || is_enumerant(parsed, text, reference.span)
                {
                    continue;
                }
                let name = parsed.text(text, reference.span);
                let continues = precedes_path_separator(text, reference.span.end);
                if reference.is_member {
                    // Only the last segment of a `::` path; a field after `.`
                    // and the path's inner segments are left alone.
                    if continues || !follows_path_separator(parsed, text, start) {
                        continue;
                    }
                    let Some(prefix) = parsed.path_prefix(text, start) else { continue };
                    let path = path_at(parsed, text, start)?;
                    let from = prefix
                        .root_span
                        .or(prefix.segments.first().map(|(_, span)| *span))
                        .map_or(start, |span| span.start);
                    targets.push((ByteSpan::new(from, reference.span.end), Target::Path(path)));
                    continue;
                }
                if continues {
                    continue;
                }
                if reference.is_declaration {
                    // A module-scope declaration's own name.
                    let declared = parsed.roots.iter().any(|&i| {
                        let symbol = &parsed.symbols[i];
                        symbol.name_span == reference.span && symbol.kind.is_declaration()
                    });
                    if declared {
                        targets.push((reference.span, Target::Local(name.to_string())));
                    }
                    continue;
                }
                let Some(index) = parsed.resolve_name(name, start) else { continue };
                let symbol = &parsed.symbols[index];
                if let Some(import) = parsed.import_for(index) {
                    let path = FullPath {
                        root: import.root,
                        segments: import
                            .segments
                            .iter()
                            .map(|(segment, _)| segment.clone())
                            .collect(),
                    };
                    targets.push((reference.span, Target::Path(path)));
                } else if symbol.parent.is_none() && symbol.kind.is_declaration() {
                    targets.push((reference.span, Target::Local(name.to_string())));
                }
            }
        }

        for (span, target) in targets {
            let (unit, name) = match target {
                Target::Local(name) => (u, name),
                Target::Path(path) => {
                    let from = self.units[u].uri.clone();
                    match self.server.wesl_resolve(&from, &path) {
                        Resolution::Item(item) => (self.unit(&item.uri)?, item.name),
                        _ => return None,
                    }
                }
            };
            // The root keeps its names: its entry points and bindings are
            // what the user is checking.
            if unit != 0 {
                let renamed = self.rename(unit, &name);
                edits.push(Edit { span, replacement: renamed });
            }
            // What the root needs it for: the use itself, when this is the root.
            let via = via.or(if u == 0 { Some(span) } else { None });
            self.include(unit, &name, via);
        }
        Some(edits)
    }

    /// The name declaration `name` of unit `u` is linked under.
    fn rename(&mut self, u: usize, name: &str) -> String {
        let unit = &self.units[u];
        let renamed = format!("{}__{name}", unit.prefix);
        if !self.names.iter().any(|(mangled, _)| *mangled == renamed) {
            self.names.push((renamed.clone(), format!("{}::{name}", unit.module)));
        }
        renamed
    }

    /// Queue the declaration `name` of unit `u` — the one conditional
    /// translation keeps, when there are several.
    fn include(&mut self, u: usize, name: &str, via: Option<ByteSpan>) {
        if u == 0 || !self.included.insert((u, name.to_string())) {
            return;
        }
        let unit = &self.units[u];
        let declaration = unit.parsed.roots.iter().map(|&i| &unit.parsed.symbols[i]).find(|symbol| {
            symbol.name == name
                && symbol.kind.is_declaration()
                && !unit.translation.is_removed(symbol.full_span)
        });
        // None survives the features: the renamed use is then undeclared,
        // which naga says, by the declaration's path.
        if let Some(declaration) = declaration {
            let span = declaration.full_span;
            self.chunks.push(Chunk { file: u, span, edits: Vec::new(), via });
        }
    }

    /// The `enable` and `requires` directives of every other module that
    /// contributed, which WGSL wants at the top and only once. Each is a
    /// chunk of its own.
    fn directives(&self) -> Vec<Chunk> {
        let root = &self.units[0];
        let mut seen: Vec<String> =
            directive_spans(root).into_iter().map(|span| normalise(root, span)).collect();
        let mut chunks = Vec::new();
        for (u, unit) in self.units.iter().enumerate().skip(1) {
            let Some(via) = self.chunks.iter().find(|chunk| chunk.file == u).map(|chunk| chunk.via)
            else {
                continue;
            };
            for span in directive_spans(unit) {
                let text = normalise(unit, span);
                if !seen.contains(&text) {
                    seen.push(text);
                    chunks.push(Chunk { file: u, span, edits: Vec::new(), via });
                }
            }
        }
        chunks
    }

    /// The linked text: directives, the root, then everything it pulled in.
    fn write(self, directives: Vec<Chunk>) -> Linked {
        let mut chunks = directives;
        chunks.extend(self.chunks);
        let mut source = String::new();
        let mut pieces: Vec<Piece> = Vec::new();

        for (index, chunk) in chunks.iter_mut().enumerate() {
            let text = &self.units[chunk.file].text;
            chunk.edits.sort_by_key(|edit| (edit.span.start, std::cmp::Reverse(edit.span.end)));
            let mut at = chunk.span.start;
            for edit in &chunk.edits {
                // Inside something already cut — a rename in a removed branch.
                if edit.span.start < at {
                    continue;
                }
                copy(&mut pieces, &mut source, index, text, ByteSpan::new(at, edit.span.start));
                if !edit.replacement.is_empty() {
                    pieces.push(Piece {
                        linked: source.len() as u32,
                        len: edit.replacement.len() as u32,
                        chunk: index,
                        original: edit.span,
                        copied: false,
                    });
                    source.push_str(&edit.replacement);
                }
                at = edit.span.end;
            }
            copy(&mut pieces, &mut source, index, text, ByteSpan::new(at, chunk.span.end));
            source.push('\n');
        }

        let files = self
            .units
            .into_iter()
            .map(|unit| LinkedFile {
                uri: unit.uri,
                text: unit.text.into_owned(),
                module: unit.module,
            })
            .collect();
        Linked { source, pieces, chunks, files, names: self.names }
    }
}

/// Copy `span` of `text` into the linked source unchanged.
fn copy(pieces: &mut Vec<Piece>, source: &mut String, chunk: usize, text: &str, span: ByteSpan) {
    if span.start >= span.end {
        return;
    }
    pieces.push(Piece {
        linked: source.len() as u32,
        len: span.end - span.start,
        chunk,
        original: span,
        copied: true,
    });
    source.push_str(&text[span.start as usize..span.end as usize]);
}

/// The argument lists of attributes whose arguments are not names in scope.
fn word_arguments(parsed: &Parsed, source: &str) -> Vec<ByteSpan> {
    let toks: Vec<_> = parsed.tokens.iter().filter(|t| !t.kind.is_trivia()).collect();
    let mut spans = Vec::new();
    for (i, token) in toks.iter().enumerate() {
        if token.kind != TokenKind::Attribute
            || !WORD_ATTRIBUTES.contains(&parsed.text(source, token.span))
            || toks.get(i + 1).is_none_or(|t| parsed.text(source, t.span) != "(")
        {
            continue;
        }
        let mut depth = 0;
        for close in &toks[i + 1..] {
            match parsed.text(source, close.span) {
                "(" => depth += 1,
                ")" => {
                    depth -= 1;
                    if depth == 0 {
                        spans.push(ByteSpan::new(toks[i + 1].span.start, close.span.end));
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    spans
}

/// Whether the name at `span` is a template-list word: `storage` in
/// `var<storage>`.
fn is_enumerant(parsed: &Parsed, source: &str, span: ByteSpan) -> bool {
    ENUMERANTS.contains(&parsed.text(source, span))
        && parsed
            .token_before(span.start)
            .is_some_and(|token| matches!(parsed.text(source, token.span), "<" | ","))
}

/// The module-scope `enable` and `requires` directives of a unit that
/// conditional translation keeps.
fn directive_spans(unit: &Unit) -> Vec<ByteSpan> {
    let (parsed, source) = (&*unit.parsed, &*unit.text);
    let toks: Vec<_> = parsed.tokens.iter().filter(|t| !t.kind.is_trivia()).collect();
    let mut spans = Vec::new();
    let mut depth = 0i32;
    let mut statement_start = true;
    for (i, token) in toks.iter().enumerate() {
        let text = parsed.text(source, token.span);
        if depth == 0 && statement_start && matches!(text, "enable" | "requires") {
            if let Some(end) = toks[i..].iter().find(|t| parsed.text(source, t.span) == ";") {
                let span = ByteSpan::new(token.span.start, end.span.end);
                if !unit.translation.is_removed(span) {
                    spans.push(span);
                }
            }
        }
        match text {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => depth -= 1,
            _ => {}
        }
        statement_start = depth == 0 && matches!(text, ";" | "}");
    }
    spans
}

/// A directive's text with its whitespace folded, for comparison.
fn normalise(unit: &Unit, span: ByteSpan) -> String {
    unit.parsed.text(&unit.text, span).split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_span_in_copied_text_maps_byte_for_byte_and_a_rewrite_maps_to_what_it_replaced() {
        let piece = |linked, len, (start, end), copied| Piece {
            linked,
            len,
            chunk: 0,
            original: ByteSpan::new(start, end),
            copied,
        };
        let linked = Linked {
            source: "fn f() { package__a__g(); }\n".to_string(),
            pieces: vec![
                piece(0, 9, (0, 9), true),
                piece(9, 13, (9, 10), false),
                piece(22, 5, (10, 15), true),
            ],
            chunks: vec![Chunk {
                file: 0,
                span: ByteSpan::new(0, 15),
                edits: Vec::new(),
                via: None,
            }],
            files: Vec::new(),
            names: vec![("package__a__g".to_string(), "package::a::g".to_string())],
        };
        assert_eq!(linked.locate(ByteSpan::new(3, 4)), Some((0, ByteSpan::new(3, 4))));
        assert_eq!(linked.locate(ByteSpan::new(9, 22)), Some((0, ByteSpan::new(9, 10))));
        assert_eq!(linked.locate(ByteSpan::new(22, 24)), Some((0, ByteSpan::new(10, 12))));
        assert_eq!(
            linked.demangle("no definition in scope for identifier: `package__a__g`"),
            "no definition in scope for identifier: `package::a::g`"
        );
    }
}
