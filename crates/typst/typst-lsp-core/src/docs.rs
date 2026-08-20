//! Doc comments, in the shape the Typst package ecosystem actually writes them.
//!
//! Typst has no language-level doc comment, so packages settled on a
//! convention — `tidy`'s — and the whole ecosystem documents itself with it:
//!
//! ```text
//! /// Draws a circle or ellipse.
//! ///
//! /// - ..points-style (coordinate, style): The position to place the circle on.
//! /// - name (none, str):
//! ///
//! /// === Styling
//! /// *Root*: `circle`
//! ///
//! /// - radius (number, array) = 1: The size of the circle's radius.
//! #let circle(..points-style, name: none, anchor: none) = { .. }
//! ```
//!
//! `typst-ide` collects the comment and shows its first sentence, and stops
//! there. The rest is what an editor needs: the entries are the parameter
//! documentation, and — for the many libraries whose functions take their real
//! arguments through a `..sink` and read them back out of a style dictionary —
//! the `Styling` section is the *only* place the accepted argument names are
//! written down. cetz's `circle` declares two named parameters and accepts a
//! dozen; `radius` lives here and nowhere else the compiler can see.
//!
//! So this module parses the convention: prose, entries, sections, and the
//! `*Root*` a styling section names. Nothing here touches the syntax tree —
//! [`crate::features::doc_params`] does that and calls in.

use ecow::EcoString;

/// One `- name (types) = default: description` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocEntry {
    /// The name, without the `..` of a sink.
    pub name: EcoString,
    /// Whether it was written `..name`.
    pub variadic: bool,
    /// The parenthesised type list, verbatim.
    pub types: Option<EcoString>,
    /// The `= default`, verbatim.
    pub default: Option<EcoString>,
    /// Everything after the colon, with continuation lines folded in.
    pub docs: EcoString,
}

impl DocEntry {
    /// The entry rendered back into one line, for a signature or a detail.
    pub fn signature(&self) -> EcoString {
        let mut out = EcoString::new();
        if self.variadic {
            out.push_str("..");
        }
        out.push_str(&self.name);
        if let Some(types) = &self.types {
            out.push_str(": ");
            out.push_str(types);
        }
        if let Some(default) = &self.default {
            out.push_str(" = ");
            out.push_str(default);
        }
        out
    }
}

/// A parsed doc comment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocComment {
    /// The whole comment, as markdown.
    pub body: EcoString,
    /// The prose before the first entry or heading.
    pub description: EcoString,
    /// The entries before the first heading: the parameters.
    pub params: Vec<DocEntry>,
    /// The style root a `Styling` section names, if it names one.
    pub style_root: Option<EcoString>,
    /// The entries inside a `Styling` section: names the function accepts
    /// through its sink.
    pub style_keys: Vec<DocEntry>,
}

/// Which list an entry belongs to.
#[derive(Clone, Copy, PartialEq)]
enum Section {
    /// Before any heading.
    Params,
    /// Under a `Styling` heading.
    Styling,
    /// Under any other heading — `Anchors`, `Examples`, prose.
    Other,
}

impl DocComment {
    /// Parse a collected doc comment.
    ///
    /// Tolerant by construction: this is a convention, not a grammar, and the
    /// packages that follow it disagree about the details. `== Styling` and
    /// `=== Styling` both occur, so does `*Root*: x` and `*Root:* x`, and half
    /// the entries have no description at all. Anything unrecognised is prose.
    pub fn parse(text: &str) -> Self {
        let mut docs = Self {
            body: text.into(),
            ..Self::default()
        };
        let mut section = Section::Params;
        // Which list the last entry went into, so continuation lines can find
        // it again.
        let mut open: Option<Section> = None;
        let mut in_code = false;

        for line in text.lines() {
            let trimmed = line.trim();

            // A fenced block is verbatim: an example that draws a `- foo`
            // list is not a parameter.
            if trimmed.starts_with("```") {
                in_code = !in_code;
                if section == Section::Params && open.is_none() {
                    docs.push_description(line);
                }
                continue;
            }
            if in_code {
                if section == Section::Params && open.is_none() {
                    docs.push_description(line);
                }
                continue;
            }

            if let Some(title) = heading(trimmed) {
                section = if title.eq_ignore_ascii_case("styling") {
                    Section::Styling
                } else {
                    Section::Other
                };
                open = None;
                continue;
            }

            if section == Section::Styling
                && let Some(root) = style_root(trimmed)
            {
                docs.style_root = Some(root);
                open = None;
                continue;
            }

            if let Some(rest) = entry_body(trimmed)
                && let Some(entry) = parse_entry(rest)
            {
                match section {
                    Section::Params => docs.params.push(entry),
                    Section::Styling => docs.style_keys.push(entry),
                    Section::Other => continue,
                }
                open = Some(section);
                continue;
            }

            if trimmed.is_empty() {
                open = None;
                if section == Section::Params {
                    docs.push_description(line);
                }
                continue;
            }

            // A wrapped description continues the entry above it.
            match open {
                Some(Section::Params) => extend(docs.params.last_mut(), trimmed),
                Some(Section::Styling) => extend(docs.style_keys.last_mut(), trimmed),
                _ if section == Section::Params => docs.push_description(line),
                _ => {}
            }
        }

        docs.description = docs.description.trim().into();
        docs
    }

    /// The documentation for a named argument, wherever the library wrote it.
    ///
    /// Not always in the parameter list: cetz's `rect` documents `name` and
    /// `anchor` under its `Styling` heading, next to the style keys.
    pub fn param(&self, name: &str) -> Option<&DocEntry> {
        self.params
            .iter()
            .chain(&self.style_keys)
            .find(|entry| entry.name == name)
    }

    /// The first sentence of the description, for a one-line detail.
    pub fn summary(&self) -> EcoString {
        first_sentence(&self.description)
    }

    /// The whole comment as markdown, for a hover.
    ///
    /// Two things in the convention are Typst markup rather than markdown and
    /// would otherwise show up as punctuation: `= Heading` and the language
    /// tag on an `example` block. Everything else already reads as markdown —
    /// the entry lists especially.
    pub fn markdown(&self) -> EcoString {
        let mut out = EcoString::new();
        let mut in_code = false;

        for line in self.body.lines() {
            let trimmed = line.trim();

            if trimmed.starts_with("```") {
                in_code = !in_code;
                // `example` is tidy's dialect for "Typst, and render it".
                if in_code && trimmed.trim_start_matches('`').starts_with("example") {
                    out.push_str("```typst");
                } else {
                    out.push_str(trimmed);
                }
            } else if !in_code && let Some(title) = heading(trimmed) {
                out.push_str("**");
                out.push_str(title);
                out.push_str("**");
            } else {
                out.push_str(line);
            }
            out.push('\n');
        }

        out
    }

    fn push_description(&mut self, line: &str) {
        self.description.push_str(line);
        self.description.push('\n');
    }
}

/// Fold a wrapped line into the entry it continues.
fn extend(entry: Option<&mut DocEntry>, text: &str) {
    let Some(entry) = entry else { return };
    if !entry.docs.is_empty() {
        entry.docs.push(' ');
    }
    entry.docs.push_str(text);
}

/// The title of a `=`-prefixed heading line.
fn heading(line: &str) -> Option<&str> {
    let rest = line.trim_start_matches('=');
    if rest.len() == line.len() {
        return None;
    }
    // `==` with nothing after it is a heading of nothing; `=x` is not a
    // heading at all.
    if !rest.is_empty() && !rest.starts_with(' ') {
        return None;
    }
    Some(rest.trim())
}

/// The root named by a `*Root*: \`circle\`` line, in any of its spellings.
fn style_root(line: &str) -> Option<EcoString> {
    let rest = line.strip_prefix("*Root")?;
    // `*Root*:`, `*Root:*`, `*Root*` — the ecosystem writes all three.
    let rest = rest.trim_start_matches([':', '*', ' ']);
    let rest = rest.strip_prefix('`')?;
    let end = rest.find('`')?;
    let root = rest[..end].trim();
    (!root.is_empty()).then(|| root.into())
}

/// The text of a `- ` list item.
fn entry_body(line: &str) -> Option<&str> {
    line.strip_prefix("- ").or_else(|| line.strip_prefix("-\t"))
}

/// `name (types) = default: docs`.
fn parse_entry(text: &str) -> Option<DocEntry> {
    let variadic = text.starts_with("..");
    let rest = text.trim_start_matches('.');

    let end = rest
        .find(|c: char| !(c.is_alphanumeric() || c == '-' || c == '_'))
        .unwrap_or(rest.len());
    let name = &rest[..end];
    if name.is_empty() {
        return None;
    }
    let mut rest = rest[end..].trim_start();

    let types = if rest.starts_with('(') {
        let end = matching(rest, '(', ')')?;
        let types = rest[1..end].trim();
        rest = rest[end + 1..].trim_start();
        Some(EcoString::from(types))
    } else {
        None
    };

    let default = if let Some(after) = rest.strip_prefix('=') {
        // The default may itself contain a colon — `= (paint: black)` — so
        // stop at the first one that is not nested.
        let end = unnested_colon(after).unwrap_or(after.len());
        let default = after[..end].trim();
        rest = &after[end..];
        Some(EcoString::from(default))
    } else {
        None
    };

    // An entry with no colon at all is still an entry; its description is
    // simply whatever is left.
    let docs = rest.strip_prefix(':').unwrap_or(rest).trim();

    Some(DocEntry {
        name: name.into(),
        variadic,
        types,
        default,
        docs: docs.into(),
    })
}

/// The index of the `close` that matches the `open` at index 0.
fn matching(text: &str, open: char, close: char) -> Option<usize> {
    let mut depth = 0usize;
    for (index, character) in text.char_indices() {
        if character == open {
            depth += 1;
        } else if character == close {
            depth -= 1;
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}

/// The index of the first `:` outside brackets, parens, and quotes.
fn unnested_colon(text: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut quoted = false;
    for (index, character) in text.char_indices() {
        match character {
            '"' => quoted = !quoted,
            _ if quoted => {}
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            ':' if depth == 0 => return Some(index),
            _ => {}
        }
    }
    None
}

/// The first sentence of a piece of prose, with the markdown taken out.
fn first_sentence(text: &str) -> EcoString {
    let paragraph = text.split("\n\n").next().unwrap_or_default();
    let mut out = EcoString::new();
    for character in paragraph.chars() {
        match character {
            '*' | '_' => {}
            '\n' => out.push(' '),
            '.' => {
                out.push('.');
                break;
            }
            _ => out.push(character),
        }
    }
    out.trim().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// cetz's `circle`, verbatim but for the trimmed examples.
    const CIRCLE: &str = "\
Draws a circle or ellipse.

```example
circle((0, 0))
```

- ..points-style (coordinate, style): The position to place the circle on.
  If given two coordinates, the distance between them is used as radius.
- name (none,str):
- anchor (none, str):

=== Styling
*Root*: `circle`

- radius (number, array) = 1: A number that defines the size of the circle's radius.

=== Anchors
  Supports border and path anchors.
";

    #[test]
    fn the_styling_section_names_the_keys_the_sink_accepts() {
        let docs = DocComment::parse(CIRCLE);
        assert_eq!(docs.style_root.as_deref(), Some("circle"));
        assert_eq!(docs.style_keys.len(), 1);

        let radius = &docs.style_keys[0];
        assert_eq!(radius.name, "radius");
        assert_eq!(radius.types.as_deref(), Some("number, array"));
        assert_eq!(radius.default.as_deref(), Some("1"));
        assert!(radius.docs.starts_with("A number that defines"));
    }

    #[test]
    fn entries_before_the_first_heading_are_the_parameters() {
        let docs = DocComment::parse(CIRCLE);
        let names: Vec<&str> = docs.params.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["points-style", "name", "anchor"]);
        assert!(docs.params[0].variadic, "`..points-style` is a sink");
        assert!(!docs.params[1].variadic);
    }

    #[test]
    fn a_wrapped_description_folds_into_the_entry_above_it() {
        let docs = DocComment::parse(CIRCLE);
        assert!(
            docs.param("points-style")
                .unwrap()
                .docs
                .ends_with("used as radius."),
            "{:?}",
            docs.param("points-style").unwrap().docs
        );
    }

    #[test]
    fn an_entry_with_no_description_still_counts() {
        let docs = DocComment::parse(CIRCLE);
        assert_eq!(docs.param("name").unwrap().docs, "");
        assert_eq!(
            docs.param("name").unwrap().types.as_deref(),
            Some("none,str")
        );
    }

    /// cetz writes `line`'s root as `*Root:*` under a `==` heading, and
    /// `circle`'s as `*Root*:` under a `===` one.
    #[test]
    fn both_spellings_of_the_root_line_are_understood() {
        let docs = DocComment::parse("== Styling\n*Root:* `line`\n\nSupports mark styling.\n");
        assert_eq!(docs.style_root.as_deref(), Some("line"));

        let docs = DocComment::parse("=== Styling\n*Root*: `arc` \\\n");
        assert_eq!(docs.style_root.as_deref(), Some("arc"));
    }

    #[test]
    fn a_default_containing_a_colon_is_not_cut_in_half() {
        let docs =
            DocComment::parse("=== Styling\n- stroke (stroke) = (paint: black): The stroke.\n");
        let stroke = &docs.style_keys[0];
        assert_eq!(stroke.default.as_deref(), Some("(paint: black)"));
        assert_eq!(stroke.docs, "The stroke.");
    }

    /// An example that draws a bullet list must not become a parameter.
    #[test]
    fn a_fenced_block_is_not_read_for_entries() {
        let docs =
            DocComment::parse("Docs.\n\n```example\n- not a param\n```\n\n- real (int): Yes.\n");
        let names: Vec<&str> = docs.params.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["real"]);
    }

    #[test]
    fn the_description_stops_at_the_first_entry() {
        let docs = DocComment::parse(CIRCLE);
        assert_eq!(docs.summary(), "Draws a circle or ellipse.");
        assert!(!docs.description.contains("points-style"));
        assert!(
            docs.description.contains("```example"),
            "examples are prose"
        );
    }

    #[test]
    fn markdown_turns_typst_markup_into_markdown() {
        let rendered = DocComment::parse(CIRCLE).markdown();
        assert!(rendered.contains("**Styling**"), "{rendered}");
        assert!(rendered.contains("**Anchors**"), "{rendered}");
        assert!(
            rendered.contains("```typst\ncircle((0, 0))\n```"),
            "{rendered}"
        );
        assert!(!rendered.contains("=== "), "{rendered}");
        // The entry lists are markdown already.
        assert!(
            rendered.contains("- radius (number, array) = 1:"),
            "{rendered}"
        );
    }

    #[test]
    fn a_comment_with_no_convention_in_it_is_just_prose() {
        let docs = DocComment::parse("Just a note.\nOn two lines.\n");
        assert!(docs.params.is_empty());
        assert!(docs.style_keys.is_empty());
        assert_eq!(docs.style_root, None);
        assert_eq!(docs.summary(), "Just a note.");
    }
}
