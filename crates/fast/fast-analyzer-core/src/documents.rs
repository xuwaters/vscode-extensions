//! Parsed virtual documents, and the UTF-16 ⇄ UTF-8 offset conversion that
//! keeps the boundary honest: the plugin speaks JavaScript string offsets,
//! the parser speaks byte offsets, and every span converts exactly once at
//! the edge of the engine.

use fast_template_syntax as syntax;

use crate::protocol::{PlaceholderFact, VirtualDocumentFact};

pub struct DocumentState {
    pub fact: VirtualDocumentFact,
    /// Parsed tree for `html` documents; `css` documents never enter the
    /// parser ([decision 0005]).
    pub tree: Option<syntax::Document>,
    /// Byte offsets of the placeholders, matching `fact.placeholders` order.
    pub placeholder_bytes: Vec<syntax::Placeholder>,
    map: OffsetMap,
    /// Any `${html.partial(…)}` in the document: analysis stops.
    pub uses_partial: bool,
}

impl DocumentState {
    pub fn new(fact: VirtualDocumentFact) -> DocumentState {
        let map = OffsetMap::new(&fact.text);
        let placeholder_bytes: Vec<syntax::Placeholder> = fact
            .placeholders
            .iter()
            .map(|p| syntax::Placeholder {
                index: p.index,
                start: map.byte_of_utf16(p.start),
                end: map.byte_of_utf16(p.end),
            })
            .collect();
        let uses_partial = fact
            .placeholders
            .iter()
            .any(|p| p.expr.as_ref().map(|e| e.is_partial).unwrap_or(false));
        let tree = if fact.kind == "html" {
            Some(syntax::parse(&fact.text, &placeholder_bytes))
        } else {
            None
        };
        DocumentState {
            fact,
            tree,
            placeholder_bytes,
            map,
            uses_partial,
        }
    }

    pub fn text(&self) -> &str {
        &self.fact.text
    }

    pub fn byte_of_utf16(&self, offset: u32) -> usize {
        self.map.byte_of_utf16(offset)
    }

    pub fn utf16_of_byte(&self, offset: usize) -> u32 {
        self.map.utf16_of_byte(offset)
    }

    pub fn utf16_span(&self, span: syntax::Span) -> (u32, u32) {
        (self.utf16_of_byte(span.start), self.utf16_of_byte(span.end))
    }

    /// The placeholder's metadata, by expression index.
    pub fn placeholder_fact(&self, index: u32) -> Option<&PlaceholderFact> {
        self.fact.placeholders.iter().find(|p| p.index == index)
    }

    /// Absolute source-file offset for a document-relative UTF-16 offset.
    pub fn absolute(&self, utf16_offset: u32) -> u32 {
        self.fact.template_start + utf16_offset
    }
}

enum OffsetMap {
    /// ASCII text: byte and UTF-16 offsets are the same thing.
    Identity,
    /// `utf16_starts[i]` is the UTF-16 offset of byte `i` for every byte at a
    /// character boundary. O(text) memory, O(1) lookups; documents are
    /// kilobytes.
    Table { utf16_of_byte: Vec<u32> },
}

impl OffsetMap {
    fn new(text: &str) -> OffsetMap {
        if text.is_ascii() {
            return OffsetMap::Identity;
        }
        let mut utf16_of_byte = vec![0u32; text.len() + 1];
        let mut utf16 = 0u32;
        for (byte_pos, ch) in text.char_indices() {
            for b in 0..ch.len_utf8() {
                utf16_of_byte[byte_pos + b] = utf16;
            }
            utf16 += ch.len_utf16() as u32;
        }
        utf16_of_byte[text.len()] = utf16;
        OffsetMap::Table { utf16_of_byte }
    }

    fn utf16_of_byte(&self, byte: usize) -> u32 {
        match self {
            OffsetMap::Identity => byte as u32,
            OffsetMap::Table { utf16_of_byte } => {
                let clamped = byte.min(utf16_of_byte.len() - 1);
                utf16_of_byte[clamped]
            }
        }
    }

    fn byte_of_utf16(&self, offset: u32) -> usize {
        match self {
            OffsetMap::Identity => offset as usize,
            OffsetMap::Table { utf16_of_byte } => {
                // The table is monotonically non-decreasing; find the first
                // byte whose UTF-16 offset matches.
                utf16_of_byte
                    .partition_point(|&u| u < offset)
                    .min(utf16_of_byte.len() - 1)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_for_ascii() {
        let map = OffsetMap::new("hello");
        assert_eq!(map.byte_of_utf16(3), 3);
        assert_eq!(map.utf16_of_byte(5), 5);
    }

    #[test]
    fn multibyte_round_trip() {
        // "a⌘b" — ⌘ is 3 bytes, 1 UTF-16 unit.
        let text = "a⌘b";
        let map = OffsetMap::new(text);
        assert_eq!(map.utf16_of_byte(0), 0);
        assert_eq!(map.utf16_of_byte(1), 1); // start of ⌘
        assert_eq!(map.utf16_of_byte(4), 2); // start of b
        assert_eq!(map.utf16_of_byte(5), 3); // end
        assert_eq!(map.byte_of_utf16(0), 0);
        assert_eq!(map.byte_of_utf16(1), 1);
        assert_eq!(map.byte_of_utf16(2), 4);
        assert_eq!(map.byte_of_utf16(3), 5);
    }

    #[test]
    fn surrogate_pairs() {
        // "🙂" is 4 bytes, 2 UTF-16 units.
        let text = "x🙂y";
        let map = OffsetMap::new(text);
        assert_eq!(map.utf16_of_byte(1), 1);
        assert_eq!(map.utf16_of_byte(5), 3); // after the emoji
        assert_eq!(map.byte_of_utf16(3), 5);
        assert_eq!(map.byte_of_utf16(4), 6);
    }
}
