//! Line-oriented tokenizer for Makefiles.
//!
//! Makefiles are structured around physical lines: a recipe line *must*
//! start with TAB, assignments and rules *must* start in column 0, and a
//! trailing `\` joins two physical lines into one logical line. This
//! module performs that joining and classifies each resulting logical
//! line into a [`LineKind`] so the parser can dispatch without rescanning.

use crate::spans::ByteSpan;

#[derive(Debug, Clone)]
pub struct LogicalLine {
    /// Joined source text, with escaped newlines replaced by a single
    /// space (matching GNU Make's behaviour for non-recipe lines). For
    /// recipe lines the original text is preserved verbatim including
    /// the leading tab.
    pub text: String,
    /// Full physical extent of this logical line in the original source,
    /// including any trailing newline that terminated it.
    pub span: ByteSpan,
    /// The first non-whitespace byte offset on the *first* physical line.
    /// Used by the parser to emit diagnostics at the right column.
    pub content_start: u32,
    pub kind: LineKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    /// Blank (whitespace-only) line.
    Blank,
    /// `# ...` comment. Leading whitespace before the `#` is tolerated.
    Comment,
    /// A line that starts with a literal TAB — this is how GNU Make
    /// decides something is a recipe, regardless of content.
    Recipe,
    /// A line that starts with whitespace other than TAB, where the
    /// whitespace is followed by something that looks like a recipe
    /// command. We flag this so the parser can emit `MAKE001`.
    RecipeWithSpaces,
    /// Something else — an assignment, a rule header, a directive, or
    /// an unknown construct. The parser is responsible for the final
    /// classification.
    Statement,
}

pub fn lex(source: &str) -> Vec<LogicalLine> {
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let mut i: usize = 0;

    while i < bytes.len() {
        let line_start = i;

        // Capture the raw first physical line (leading indent included)
        // so we can preserve recipe tabs byte-for-byte.
        let first_line_end = find_line_end(bytes, i);
        let first_line_slice = &source[line_start..first_line_end];

        // A recipe line is only a recipe if the *first* physical line of
        // the logical line starts with a TAB. GNU Make does not continue
        // recipe-ness across joined lines.
        let starts_with_tab = first_line_slice.starts_with('\t');

        // Join escaped-newline continuations.
        let mut joined = String::new();
        let mut physical_end = first_line_end;
        let mut line_i = line_start;
        let mut physical_slice_end = first_line_end;

        loop {
            let slice = &source[line_i..physical_slice_end];
            // Is this physical line continued? A trailing `\` with an
            // *odd* count of backslashes right before the newline means
            // continuation.
            let (content_end, cont) = strip_continuation(slice);
            if !joined.is_empty() {
                // For the second and later physical lines, strip leading
                // whitespace — that matches GNU Make's join behaviour.
                let trimmed = slice[..content_end].trim_start();
                if !joined.ends_with(' ') {
                    joined.push(' ');
                }
                joined.push_str(trimmed);
            } else {
                joined.push_str(&slice[..content_end]);
            }

            if !cont {
                break;
            }
            // Move to the next physical line.
            if physical_slice_end >= bytes.len() {
                break;
            }
            // Skip the newline.
            let next_start = skip_newline(bytes, physical_slice_end);
            if next_start >= bytes.len() {
                physical_end = bytes.len();
                break;
            }
            line_i = next_start;
            physical_slice_end = find_line_end(bytes, next_start);
            physical_end = physical_slice_end;
        }

        // Advance past the terminating newline so the next loop picks up
        // the following line.
        let after_newline = skip_newline(bytes, physical_end);

        let (content_start, kind) =
            classify(source, line_start, &joined, starts_with_tab);

        out.push(LogicalLine {
            text: joined,
            span: ByteSpan::from_usize(line_start, after_newline),
            content_start,
            kind,
        });

        i = after_newline;
    }

    out
}

/// Position of the next `\n` or end-of-buffer. The returned offset is
/// *before* the newline character.
fn find_line_end(bytes: &[u8], from: usize) -> usize {
    let mut i = from;
    while i < bytes.len() && bytes[i] != b'\n' {
        i += 1;
    }
    i
}

/// Consume a trailing `\r\n` or `\n`. Returns the offset *after* the
/// newline sequence.
fn skip_newline(bytes: &[u8], at: usize) -> usize {
    if at < bytes.len() && bytes[at] == b'\n' {
        at + 1
    } else {
        at
    }
}

/// Given a physical-line slice (no trailing newline), determine whether
/// it ends with a continuation backslash. Returns `(end_before_backslash,
/// is_continued)`. Strips trailing `\r` from the end slice.
fn strip_continuation(slice: &str) -> (usize, bool) {
    let bytes = slice.as_bytes();
    let mut end = bytes.len();
    if end > 0 && bytes[end - 1] == b'\r' {
        end -= 1;
    }
    // Count trailing backslashes.
    let mut backslashes = 0usize;
    let mut j = end;
    while j > 0 && bytes[j - 1] == b'\\' {
        j -= 1;
        backslashes += 1;
    }
    let cont = backslashes % 2 == 1;
    if cont {
        // Drop the continuation backslash from the logical text.
        (end - 1, true)
    } else {
        (end, false)
    }
}

fn classify(
    source: &str,
    line_start: usize,
    joined: &str,
    starts_with_tab: bool,
) -> (u32, LineKind) {
    // content_start is the absolute byte offset of the first
    // non-whitespace character on the first physical line.
    let first_line_slice = source[line_start..]
        .split_once('\n')
        .map(|(a, _)| a)
        .unwrap_or(&source[line_start..]);
    let leading_ws = first_line_slice
        .bytes()
        .take_while(|b| matches!(b, b' ' | b'\t'))
        .count();
    let content_start = (line_start + leading_ws) as u32;

    let trimmed = joined.trim_start();

    if trimmed.is_empty() {
        return (content_start, LineKind::Blank);
    }
    if trimmed.starts_with('#') {
        return (content_start, LineKind::Comment);
    }

    if starts_with_tab {
        // GNU Make: lines that begin with TAB are recipe lines, but a
        // lone TAB followed by nothing or a comment is conventionally
        // just a blank line. For outline purposes we still record them
        // as Recipe — they belong to the enclosing rule.
        return (content_start, LineKind::Recipe);
    }

    // A line that starts with spaces but then "looks like" a recipe
    // command (non-directive, non-assignment, non-rule) is probably a
    // tab-vs-space mistake. We don't definitively classify it here —
    // the parser owns that decision once it knows the surrounding
    // context. For now we treat it as a regular statement but mark
    // leading-space-only indentation in a dedicated variant so the
    // parser can promote it to a diagnostic when it appears right
    // after a rule header.
    let raw_first = &source[line_start..];
    let leading_is_spaces_only = raw_first
        .bytes()
        .take(leading_ws)
        .all(|b| b == b' ');
    if leading_ws > 0 && leading_is_spaces_only {
        return (content_start, LineKind::RecipeWithSpaces);
    }

    (content_start, LineKind::Statement)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_basic_lines() {
        let src = "# hello\nall: dep\n\techo hi\n\nVAR = 1\n";
        let lines = lex(src);
        assert_eq!(lines.len(), 5);
        assert_eq!(lines[0].kind, LineKind::Comment);
        assert_eq!(lines[1].kind, LineKind::Statement);
        assert_eq!(lines[2].kind, LineKind::Recipe);
        assert_eq!(lines[3].kind, LineKind::Blank);
        assert_eq!(lines[4].kind, LineKind::Statement);
    }

    #[test]
    fn joins_continuations() {
        let src = "SOURCES = a.c \\\n  b.c \\\n  c.c\n";
        let lines = lex(src);
        assert_eq!(lines.len(), 1);
        // Leading whitespace on continued lines is stripped and replaced
        // by a single separating space, matching GNU Make's behaviour
        // for non-recipe lines.
        assert_eq!(lines[0].text, "SOURCES = a.c b.c c.c");
    }

    #[test]
    fn recipe_continuation_preserves_first_tab() {
        let src = "all:\n\techo hi \\\n\t     there\n";
        let lines = lex(src);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1].kind, LineKind::Recipe);
        assert!(lines[1].text.starts_with('\t'));
    }

    #[test]
    fn detects_space_indented_recipe() {
        let src = "all:\n  echo hi\n";
        let lines = lex(src);
        assert_eq!(lines[1].kind, LineKind::RecipeWithSpaces);
    }
}
