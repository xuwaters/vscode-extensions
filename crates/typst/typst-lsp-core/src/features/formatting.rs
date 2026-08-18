//! Formatting via `typstyle-core`, pinned to the compiler's version.
//!
//! `render()` returns `Err(SyntaxError)` when the document has parse errors. We
//! answer `null` in that case rather than mangling the file — a formatter that
//! reformats a half-typed document into something else is a formatter people
//! turn off.

use lsp_types::{
    DocumentFormattingParams, DocumentRangeFormattingParams, Position, Range, TextEdit,
};
use typst::syntax::Source;
use typstyle_core::{Config, Typstyle};

use crate::convert::{range_from_lsp, range_to_lsp};
use crate::settings::{FormatterMode, FormatterSettings};
use crate::{Ports, Server};

impl<Q: Ports> Server<Q> {
    /// `textDocument/formatting`.
    pub fn formatting(&mut self, params: DocumentFormattingParams) -> Option<Vec<TextEdit>> {
        let settings = self.settings().formatter;
        if settings.mode == FormatterMode::Off {
            return None;
        }

        let (_, source) = self.source_of(&params.text_document.uri)?;
        let formatted = format_document(&source, &settings)?;
        if formatted == source.text() {
            return Some(Vec::new());
        }

        Some(vec![TextEdit {
            range: whole_document(&source),
            new_text: formatted,
        }])
    }

    /// `textDocument/rangeFormatting`, via `typstyle_core::partial`.
    pub fn range_formatting(
        &mut self,
        params: DocumentRangeFormattingParams,
    ) -> Option<Vec<TextEdit>> {
        let settings = self.settings().formatter;
        if settings.mode == FormatterMode::Off {
            return None;
        }

        let (_, source) = self.source_of(&params.text_document.uri)?;
        let byte_range = range_from_lsp(&source, params.range);

        let result = Typstyle::new(config(&settings))
            .format_source_range(source.clone(), byte_range)
            .ok()?;

        // The formatter widens the range to whole syntax nodes, so the edit has
        // to be reported against the range it actually rewrote.
        let existing = source.text().get(result.source_range.clone())?;
        if existing == result.content {
            return Some(Vec::new());
        }

        Some(vec![TextEdit {
            range: range_to_lsp(&source, result.source_range),
            new_text: result.content,
        }])
    }
}

/// Format a whole document, or `None` if it does not parse.
pub fn format_document(source: &Source, settings: &FormatterSettings) -> Option<String> {
    Typstyle::new(config(settings)).format_source(source.clone()).render().ok()
}

fn config(settings: &FormatterSettings) -> Config {
    Config::default()
        .with_width(settings.print_width)
        .with_tab_spaces(settings.indent_size)
}

fn whole_document(source: &Source) -> Range {
    let lines = source.lines();
    let last_line = lines.len_lines().saturating_sub(1);
    Range {
        start: Position { line: 0, character: 0 },
        end: Position {
            line: last_line as u32,
            character: line_length_utf16(source, last_line) as u32,
        },
    }
}

fn line_length_utf16(source: &Source, line: usize) -> usize {
    let lines = source.lines();
    let Some(range) = lines.line_to_range(line) else { return 0 };
    let text = source.text().get(range).unwrap_or_default();
    text.trim_end_matches(['\n', '\r']).chars().map(char::len_utf16).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_well_formed_document_formats() {
        let source = Source::detached("#let   f(x)   =   x+1\n");
        let formatted = format_document(&source, &FormatterSettings::default()).unwrap();
        assert_eq!(formatted.trim(), "#let f(x) = x + 1");
    }

    #[test]
    fn a_document_with_syntax_errors_is_left_alone() {
        let source = Source::detached("#let x = (1, 2\n");
        assert!(
            format_document(&source, &FormatterSettings::default()).is_none(),
            "formatting a broken document must not mangle it"
        );
    }

    #[test]
    fn the_print_width_setting_is_honoured() {
        let long = format!("#let xs = ({})\n", (0..30).map(|n| n.to_string()).collect::<Vec<_>>().join(", "));
        let source = Source::detached(long.as_str());

        let narrow = FormatterSettings { print_width: 40, ..FormatterSettings::default() };
        let wide = FormatterSettings { print_width: 400, ..FormatterSettings::default() };

        let narrow_out = format_document(&source, &narrow).unwrap();
        let wide_out = format_document(&source, &wide).unwrap();

        assert!(narrow_out.lines().count() > wide_out.lines().count());
    }
}
