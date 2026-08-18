//! Byte offsets ⇄ LSP positions, and file ids ⇄ URIs.
//!
//! Typst works in **byte offsets**; LSP works in **UTF-16 code units**. This is
//! the single place in the crate that translates. Getting it wrong produces
//! off-by-one squiggles that are maddening to debug from the outside, so it is
//! isolated here with a property test rather than inlined at twenty call sites.

use std::ops::Range;

use lsp_types::{Position, Range as LspRange, Uri};
use percent_encoding::{AsciiSet, CONTROLS, percent_decode_str, utf8_percent_encode};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};

/// Characters that must be escaped in a `file:` URI path segment.
///
/// Deliberately conservative: everything outside the unreserved set plus `/`,
/// which is a separator here rather than data.
const PATH_ESCAPES: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'`')
    .add(b'{')
    .add(b'}')
    .add(b'%')
    .add(b'\\')
    .add(b'^')
    .add(b'|');

/// Byte offset → LSP position within a source file.
pub fn offset_to_position(source: &Source, offset: usize) -> Position {
    let lines = source.lines();
    let offset = offset.min(source.text().len());
    let line = lines.byte_to_line(offset).unwrap_or(0);
    let line_start = lines.line_to_byte(line).unwrap_or(0);

    // Both ends are absolute UTF-16 indices; LSP wants the difference.
    let start_utf16 = lines.byte_to_utf16(line_start).unwrap_or(0);
    let offset_utf16 = lines.byte_to_utf16(offset).unwrap_or(start_utf16);

    Position {
        line: line as u32,
        character: (offset_utf16 - start_utf16) as u32,
    }
}

/// LSP position → byte offset within a source file.
///
/// Clamps rather than failing: clients legitimately send a character index past
/// the end of a line (VSCode does it when the cursor is in virtual space), and
/// refusing those turns a cosmetic mismatch into a broken request.
pub fn position_to_offset(source: &Source, position: Position) -> usize {
    let lines = source.lines();
    let text = source.text();

    let line = position.line as usize;
    let Some(line_range) = lines.line_to_range(line) else {
        return text.len();
    };

    // `line_to_range` includes the line terminator, which is not a column the
    // cursor can sit at — clamping to it would land on the next line.
    let line_end = trim_line_end(text, line_range.clone());

    let line_start_utf16 = lines.byte_to_utf16(line_range.start).unwrap_or(0);
    let target_utf16 = line_start_utf16 + position.character as usize;

    match lines.utf16_to_byte(target_utf16) {
        Some(offset) if offset <= line_end => offset,
        _ => line_end,
    }
}

/// A line's range without its trailing `\n` or `\r\n`.
fn trim_line_end(text: &str, range: Range<usize>) -> usize {
    let mut end = range.end;
    while end > range.start && matches!(text.as_bytes().get(end - 1), Some(b'\n' | b'\r')) {
        end -= 1;
    }
    end
}

/// Byte range → LSP range.
pub fn range_to_lsp(source: &Source, range: Range<usize>) -> LspRange {
    LspRange {
        start: offset_to_position(source, range.start),
        end: offset_to_position(source, range.end),
    }
}

/// LSP range → byte range.
pub fn range_from_lsp(source: &Source, range: LspRange) -> Range<usize> {
    let start = position_to_offset(source, range.start);
    let end = position_to_offset(source, range.end);
    start..end.max(start)
}

/// Maps between workspace URIs and typst file ids.
///
/// Project files hang off the compile root; package files hang off the package
/// cache directory, which the host reports at startup because only it knows
/// where typst-cli keeps things on this platform.
#[derive(Debug, Clone)]
pub struct UriMap {
    /// `file:///path/to/project`, no trailing slash.
    root: String,
    /// `file:///home/u/.cache/typst/packages`, no trailing slash.
    package_root: Option<String>,
}

impl UriMap {
    /// Build a map for a compile root, with an optional package cache.
    pub fn new(root_uri: &str, package_cache_uri: Option<&str>) -> Self {
        Self {
            root: root_uri.trim_end_matches('/').to_string(),
            package_root: package_cache_uri.map(|uri| uri.trim_end_matches('/').to_string()),
        }
    }

    /// The compile root URI.
    pub fn root_uri(&self) -> &str {
        &self.root
    }

    /// Resolve a document URI to a file id.
    ///
    /// Returns `None` for files outside the compile root and outside the
    /// package cache — the "not in project" case decision 0008 describes.
    pub fn to_file_id(&self, uri: &Uri) -> Option<FileId> {
        let uri = decode(uri.as_str());

        if let Some(rest) = strip_root(&uri, &decode(&self.root)) {
            let vpath = VirtualPath::new(rest).ok()?;
            return Some(FileId::new(RootedPath::new(VirtualRoot::Project, vpath)));
        }

        let package_root = decode(self.package_root.as_deref()?);
        let rest = strip_root(&uri, &package_root)?;

        // `<namespace>/<name>/<version>/<path…>`
        let mut parts = rest.splitn(4, '/');
        let namespace = parts.next()?;
        let name = parts.next()?;
        let version = parts.next()?;
        let path = parts.next()?;

        let spec = format!("@{namespace}/{name}:{version}")
            .parse::<typst::syntax::package::PackageSpec>()
            .ok()?;
        let vpath = VirtualPath::new(path).ok()?;
        Some(FileId::new(RootedPath::new(VirtualRoot::Package(spec), vpath)))
    }

    /// Resolve a file id back to a document URI.
    pub fn to_uri(&self, id: FileId) -> Option<Uri> {
        let path = id.get();
        let vpath = path.vpath().get_without_slash();

        let text = match path.root() {
            VirtualRoot::Project => format!("{}/{}", self.root, encode(vpath)),
            VirtualRoot::Package(spec) => format!(
                "{}/{}/{}/{}/{}",
                self.package_root.as_ref()?,
                encode(&spec.namespace),
                encode(&spec.name),
                encode(&spec.version.to_string()),
                encode(vpath),
            ),
        };

        text.parse().ok()
    }
}

fn strip_root<'a>(uri: &'a str, root: &str) -> Option<&'a str> {
    if root.is_empty() {
        return None;
    }
    let rest = uri.strip_prefix(root)?;
    // A prefix match must land on a separator, or `/proj` would swallow
    // `/project-two/main.typ`.
    rest.strip_prefix('/').filter(|rest| !rest.is_empty())
}

fn decode(text: &str) -> String {
    percent_decode_str(text).decode_utf8_lossy().into_owned()
}

fn encode(text: &str) -> String {
    utf8_percent_encode(text, PATH_ESCAPES).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(text: &str) -> Source {
        Source::detached(text)
    }

    /// P1-07's property test. Every byte offset that lands on a character
    /// boundary must survive a round trip through LSP's UTF-16 coordinates —
    /// including through characters that are two UTF-16 units (CJK is one unit,
    /// emoji are two), grapheme clusters joined by ZWJ, and combining marks that
    /// look like one character but are several.
    #[test]
    fn every_offset_round_trips() {
        let documents = [
            "= Heading\n\nPlain ASCII body text.\n",
            "= 標題\n\n中文段落，包含標點符號。\n第二行\n",
            "Emoji: 👩‍💻 and 🇯🇵 and 👨‍👩‍👧‍👦 mixed with text\n",
            "Combining: e\u{0301}a\u{0308}o\u{0303} and precomposed éäõ\n",
            "Mixed 中文 with 👩‍💻 and e\u{0301} on one line\nsecond 行\n",
            "trailing newline absent",
            "",
            "\n\n\n",
        ];

        for text in documents {
            let source = source(text);
            for offset in 0..=text.len() {
                if !text.is_char_boundary(offset) {
                    continue;
                }
                let position = offset_to_position(&source, offset);
                let back = position_to_offset(&source, position);
                assert_eq!(
                    back, offset,
                    "offset {offset} in {text:?} round-tripped to {back} via {position:?}"
                );
            }
        }
    }

    #[test]
    fn a_character_index_past_the_end_of_a_line_clamps_to_it() {
        let source = source("ab\ncd\n");
        let offset = position_to_offset(&source, Position { line: 0, character: 99 });
        assert_eq!(offset, 2, "should clamp to the end of line 0, not run on");
    }

    #[test]
    fn a_line_past_the_end_clamps_to_the_document() {
        let source = source("ab\n");
        let offset = position_to_offset(&source, Position { line: 99, character: 0 });
        assert_eq!(offset, source.text().len());
    }

    #[test]
    fn astral_characters_count_as_two_utf16_units() {
        let source = source("a👩b\n");
        // "a" = 1 unit, the emoji = 2 units, so "b" starts at character 3.
        let position = offset_to_position(&source, "a👩".len());
        assert_eq!(position, Position { line: 0, character: 3 });
    }

    #[test]
    fn project_uris_map_to_project_file_ids_and_back() {
        let map = UriMap::new("file:///home/u/proj", None);
        let uri: Uri = "file:///home/u/proj/chapters/one.typ".parse().unwrap();

        let id = map.to_file_id(&uri).expect("inside the root");
        assert_eq!(id.get().vpath().get_with_slash(), "/chapters/one.typ");
        assert_eq!(map.to_uri(id).unwrap().as_str(), uri.as_str());
    }

    #[test]
    fn a_uri_outside_the_root_has_no_file_id() {
        let map = UriMap::new("file:///home/u/proj", None);
        let outside: Uri = "file:///home/u/elsewhere/main.typ".parse().unwrap();
        assert!(map.to_file_id(&outside).is_none());

        // A shared prefix that is not a path boundary must not match either.
        let sibling: Uri = "file:///home/u/proj-two/main.typ".parse().unwrap();
        assert!(map.to_file_id(&sibling).is_none());
    }

    #[test]
    fn percent_encoded_paths_survive_both_directions() {
        let map = UriMap::new("file:///home/u/my%20proj", None);
        let uri: Uri = "file:///home/u/my%20proj/a%20file.typ".parse().unwrap();

        let id = map.to_file_id(&uri).expect("spaces are still inside the root");
        assert_eq!(id.get().vpath().get_with_slash(), "/a file.typ");
        assert_eq!(map.to_uri(id).unwrap().as_str(), uri.as_str());
    }

    #[test]
    fn package_files_map_through_the_cache_directory() {
        let map = UriMap::new(
            "file:///home/u/proj",
            Some("file:///home/u/.cache/typst/packages"),
        );
        let uri: Uri = "file:///home/u/.cache/typst/packages/preview/cetz/0.4.2/src/lib.typ"
            .parse()
            .unwrap();

        let id = map.to_file_id(&uri).expect("inside the package cache");
        let VirtualRoot::Package(spec) = id.get().root() else {
            panic!("expected a package root");
        };
        assert_eq!(spec.to_string(), "@preview/cetz:0.4.2");
        assert_eq!(map.to_uri(id).unwrap().as_str(), uri.as_str());
    }
}
