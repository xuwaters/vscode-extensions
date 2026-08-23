//! Tolerant tokenizer and tree for interpolated HTML.
//!
//! The input is a FAST Element template *after* the virtual-document
//! substitution: every `${…}` in the source has been replaced by a
//! length-preserving underscore run, and the caller passes the placeholder
//! table alongside the text. The parser consults the table by offset — it
//! never pattern-matches the underscores — so a placeholder is recognised in
//! any position: content, attribute value, or attribute-name position (the
//! element-expression case, `<div ${ref('el')}>`).
//!
//! Deliberate divergences from an HTML5 parser, all in service of diagnostics:
//!
//! - **Unclosed tags stay unclosed.** A spec parser repairs the tree; this one
//!   records what was written. Elements whose end tag is optional in HTML
//!   (`<li>`, `<p>`, `<td>`, …) are closed implicitly and marked as such, so
//!   `no-unclosed-tag` can tell a legal omission from a mistake.
//! - **Self-closing is legal only in foreign content** (`<svg>`, `<math>`).
//!   `<div/>` and `<my-el/>` parse as closed but carry an error.
//! - **No fostering, no breakout.** Table-content fostering and foreign-content
//!   breakout cannot occur in a template a browser would render as written;
//!   the differential test against parse5 is what checks that claim.
//!
//! All offsets are byte offsets into the caller's `&str`. The crate knows
//! nothing about tagged templates, TypeScript, or UTF-16 — the adapter above
//! it owns coordinate conversion.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Span {
        Span { start, end }
    }

    pub fn contains(&self, offset: usize) -> bool {
        self.start <= offset && offset < self.end
    }

    /// Inclusive-end containment, for cursor positions that sit just past a
    /// token — hovering the last character of a name included.
    pub fn touches(&self, offset: usize) -> bool {
        self.start <= offset && offset <= self.end
    }

    pub fn text<'a>(&self, text: &'a str) -> &'a str {
        &text[self.start.min(text.len())..self.end.min(text.len())]
    }
}

/// One `${…}` in the source, as the substitution left it: `start..end` covers
/// the full expression *including* the `${` and `}` delimiters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placeholder {
    pub index: u32,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaceholderRef {
    pub index: u32,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementKind {
    Html,
    Custom,
    Svg,
    MathMl,
}

/// The binding-aspect prefix on an attribute name: `:` property, `?` boolean
/// attribute, `@` event. The prefix character carries its own span so a
/// diagnostic can point at the modifier rather than the whole name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier {
    Property,
    Boolean,
    Event,
}

impl Modifier {
    pub fn char(&self) -> char {
        match self {
            Modifier::Property => ':',
            Modifier::Boolean => '?',
            Modifier::Event => '@',
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quote {
    Double,
    Single,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttrPart {
    Literal(Span),
    Placeholder(PlaceholderRef),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeValue {
    pub quote: Option<Quote>,
    /// Inside the quotes (or the bare value when unquoted).
    pub span: Span,
    pub parts: Vec<AttrPart>,
}

impl AttributeValue {
    pub fn single_placeholder(&self) -> Option<&PlaceholderRef> {
        match self.parts.as_slice() {
            [AttrPart::Placeholder(p)] => Some(p),
            _ => None,
        }
    }

    pub fn placeholders(&self) -> impl Iterator<Item = &PlaceholderRef> {
        self.parts.iter().filter_map(|p| match p {
            AttrPart::Placeholder(p) => Some(p),
            AttrPart::Literal(_) => None,
        })
    }

    pub fn is_literal_only(&self) -> bool {
        self.parts.iter().all(|p| matches!(p, AttrPart::Literal(_)))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribute {
    pub modifier: Option<(Modifier, Span)>,
    /// The name without its modifier character.
    pub name: Span,
    /// Modifier, name, `=` and value together.
    pub full: Span,
    pub value: Option<AttributeValue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CloseTag {
    /// `</name>` in full.
    pub span: Span,
    pub name: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    pub name: Span,
    /// `<` through the `>` that ends the open tag (or EOF when never closed).
    pub open: Span,
    pub close: Option<CloseTag>,
    pub self_closing: bool,
    pub kind: ElementKind,
    pub is_void: bool,
    pub attributes: Vec<Attribute>,
    /// Placeholders in attribute-name position — `<div ${ref('el')}>`.
    pub element_expressions: Vec<PlaceholderRef>,
    pub children: Vec<Node>,
    /// Closed by an implied end tag or by end-of-input where HTML allows the
    /// omission — legal, and distinct from unclosed.
    pub closed_implicitly: bool,
}

impl Element {
    pub fn name_text<'a>(&self, text: &'a str) -> &'a str {
        self.name.text(text)
    }

    /// Where content ends: the start of the close tag, or the end of the last
    /// child, or the end of the open tag.
    pub fn inner_end(&self) -> usize {
        if let Some(close) = &self.close {
            return close.span.start;
        }
        match self.children.last() {
            Some(child) => child.span().end,
            None => self.open.end,
        }
    }

    pub fn span(&self) -> Span {
        let end = self
            .close
            .map(|c| c.span.end)
            .unwrap_or_else(|| self.inner_end());
        Span::new(self.open.start, end.max(self.open.end))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Element(Element),
    Text(Span),
    Comment(Span),
    Placeholder(PlaceholderRef),
    Doctype(Span),
}

impl Node {
    pub fn span(&self) -> Span {
        match self {
            Node::Element(e) => e.span(),
            Node::Text(s) | Node::Comment(s) | Node::Doctype(s) => *s,
            Node::Placeholder(p) => p.span,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseErrorKind {
    /// A non-optional element was never closed.
    UnclosedTag,
    /// `<div/>` or `<my-el/>` outside foreign content.
    SelfClosedNonVoid,
    /// A close tag with no matching open tag.
    StrayCloseTag,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub kind: ParseErrorKind,
    /// Where to report: the offending tag's name.
    pub span: Span,
    pub tag: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Document {
    pub children: Vec<Node>,
    pub errors: Vec<ParseError>,
}

impl Document {
    /// Depth-first walk over every element.
    pub fn visit_elements<'a>(&'a self, f: &mut impl FnMut(&'a Element, Option<&'a Element>)) {
        fn walk<'a>(
            nodes: &'a [Node],
            parent: Option<&'a Element>,
            f: &mut impl FnMut(&'a Element, Option<&'a Element>),
        ) {
            for node in nodes {
                if let Node::Element(el) = node {
                    f(el, parent);
                    walk(&el.children, Some(el), f);
                }
            }
        }
        walk(&self.children, None, f);
    }
}

pub const VOID_ELEMENTS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta",
    "param", "source", "track", "wbr",
];

pub fn is_void(name: &str) -> bool {
    VOID_ELEMENTS.iter().any(|v| name.eq_ignore_ascii_case(v))
}

const RAW_TEXT_ELEMENTS: &[&str] = &["style", "script", "textarea", "title"];

/// Elements whose end tag HTML lets you omit. Unclosed at EOF or closed by a
/// parent's end tag, these are legal, not mistakes.
const OPTIONAL_END_ELEMENTS: &[&str] = &[
    "html", "head", "body", "p", "li", "dt", "dd", "option", "optgroup",
    "thead", "tbody", "tfoot", "tr", "td", "th", "caption", "colgroup", "rp",
    "rt",
];

fn has_optional_end(name: &str) -> bool {
    OPTIONAL_END_ELEMENTS
        .iter()
        .any(|v| name.eq_ignore_ascii_case(v))
}

/// Blocks whose open tag implies the end of an open `<p>`.
const CLOSES_P: &[&str] = &[
    "address", "article", "aside", "blockquote", "details", "div", "dl",
    "fieldset", "figcaption", "figure", "footer", "form", "h1", "h2", "h3",
    "h4", "h5", "h6", "header", "hgroup", "hr", "main", "menu", "nav", "ol",
    "p", "pre", "section", "table", "ul",
];

/// Does opening `next` imply the end of a currently-open `open`?
fn implies_end(open: &str, next: &str) -> bool {
    let open = open.to_ascii_lowercase();
    let next = next.to_ascii_lowercase();
    match open.as_str() {
        "p" => CLOSES_P.contains(&next.as_str()),
        "li" => next == "li",
        "dt" | "dd" => next == "dt" || next == "dd",
        "option" => next == "option" || next == "optgroup",
        "optgroup" => next == "optgroup",
        "tr" => next == "tr",
        "td" | "th" => next == "td" || next == "th" || next == "tr",
        "thead" => next == "tbody" || next == "tfoot",
        "tbody" => next == "tbody" || next == "tfoot",
        "rp" | "rt" => next == "rp" || next == "rt",
        "caption" | "colgroup" => {
            next == "thead" || next == "tbody" || next == "tfoot" || next == "tr"
        }
        _ => false,
    }
}

pub fn parse(text: &str, placeholders: &[Placeholder]) -> Document {
    Parser::new(text, placeholders).run()
}

struct Parser<'a> {
    text: &'a str,
    bytes: &'a [u8],
    pos: usize,
    /// Sorted by `start`.
    placeholders: Vec<Placeholder>,
    /// Index of the first placeholder at or after `pos`.
    next_placeholder: usize,
    stack: Vec<Element>,
    document: Vec<Node>,
    errors: Vec<ParseError>,
    /// Depth of `<svg>`/`<math>` elements on the stack.
    foreign_depth: usize,
}

impl<'a> Parser<'a> {
    fn new(text: &'a str, placeholders: &'a [Placeholder]) -> Parser<'a> {
        let mut sorted: Vec<Placeholder> = placeholders.to_vec();
        sorted.sort_by_key(|p| p.start);
        Parser {
            text,
            bytes: text.as_bytes(),
            pos: 0,
            placeholders: sorted,
            next_placeholder: 0,
            stack: Vec::new(),
            document: Vec::new(),
            errors: Vec::new(),
            foreign_depth: 0,
        }
    }

    fn run(mut self) -> Document {
        while self.pos < self.bytes.len() {
            self.step();
        }
        while let Some(el) = self.stack.pop() {
            self.finish_unclosed(el);
        }
        Document {
            children: self.document,
            errors: self.errors,
        }
    }

    // ------------------------------------------------------------- utilities

    fn placeholder_at(&mut self, pos: usize) -> Option<Placeholder> {
        while self.next_placeholder < self.placeholders.len()
            && self.placeholders[self.next_placeholder].start < pos
        {
            self.next_placeholder += 1;
        }
        let candidate = self.placeholders.get(self.next_placeholder)?;
        (candidate.start == pos).then_some(*candidate)
    }

    /// The offset of the next placeholder at or after `pos`, for bounding
    /// literal runs.
    fn next_placeholder_start(&self, pos: usize) -> usize {
        self.placeholders[self.next_placeholder..]
            .iter()
            .find(|p| p.start >= pos)
            .map(|p| p.start)
            .unwrap_or(usize::MAX)
    }

    fn byte(&self, pos: usize) -> u8 {
        self.bytes.get(pos).copied().unwrap_or(0)
    }

    fn starts_with_ci(&self, pos: usize, prefix: &str) -> bool {
        let end = pos + prefix.len();
        end <= self.bytes.len() && self.text[pos..end].eq_ignore_ascii_case(prefix)
    }

    fn skip_whitespace(&mut self) {
        while self.byte(self.pos).is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn in_foreign_content(&self) -> bool {
        self.foreign_depth > 0
    }

    fn push_node(&mut self, node: Node) {
        match self.stack.last_mut() {
            Some(top) => top.children.push(node),
            None => self.document.push(node),
        }
    }

    fn is_name_byte(b: u8) -> bool {
        b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b':' || b == b'.'
    }

    // ------------------------------------------------------------------ data

    fn step(&mut self) {
        if let Some(ph) = self.placeholder_at(self.pos) {
            self.pos = ph.end;
            self.push_node(Node::Placeholder(PlaceholderRef {
                index: ph.index,
                span: Span::new(ph.start, ph.end),
            }));
            return;
        }
        if self.byte(self.pos) == b'<' {
            let next = self.byte(self.pos + 1);
            if next == b'/' {
                self.parse_close_tag();
                return;
            }
            if next.is_ascii_alphabetic() {
                self.parse_open_tag();
                return;
            }
            if self.starts_with_ci(self.pos, "<!--") {
                self.parse_comment();
                return;
            }
            if self.starts_with_ci(self.pos, "<!doctype") {
                self.parse_doctype();
                return;
            }
            if next == b'!' {
                self.parse_bogus_comment();
                return;
            }
        }
        self.parse_text();
    }

    fn parse_text(&mut self) {
        let start = self.pos;
        let limit = self.next_placeholder_start(self.pos);
        self.pos += 1; // consume at least the current character
        while self.pos < self.bytes.len() && self.pos < limit {
            let b = self.byte(self.pos);
            if b == b'<' {
                let next = self.byte(self.pos + 1);
                if next == b'/' || next == b'!' || next.is_ascii_alphabetic() {
                    break;
                }
            }
            self.pos += 1;
        }
        self.push_node(Node::Text(Span::new(start, self.pos)));
    }

    fn parse_comment(&mut self) {
        let start = self.pos;
        self.pos += 4; // <!--
        let end = match self.text[self.pos..].find("-->") {
            Some(rel) => {
                self.pos += rel + 3;
                self.pos
            }
            None => {
                self.pos = self.bytes.len();
                self.pos
            }
        };
        self.push_node(Node::Comment(Span::new(start, end)));
    }

    fn parse_doctype(&mut self) {
        let start = self.pos;
        while self.pos < self.bytes.len() && self.byte(self.pos) != b'>' {
            self.pos += 1;
        }
        if self.pos < self.bytes.len() {
            self.pos += 1;
        }
        self.push_node(Node::Doctype(Span::new(start, self.pos)));
    }

    fn parse_bogus_comment(&mut self) {
        let start = self.pos;
        while self.pos < self.bytes.len() && self.byte(self.pos) != b'>' {
            self.pos += 1;
        }
        if self.pos < self.bytes.len() {
            self.pos += 1;
        }
        self.push_node(Node::Comment(Span::new(start, self.pos)));
    }

    // ------------------------------------------------------------- open tags

    fn parse_open_tag(&mut self) {
        let tag_start = self.pos;
        self.pos += 1; // <
        let name_start = self.pos;
        while Self::is_name_byte(self.byte(self.pos)) {
            self.pos += 1;
        }
        let name = Span::new(name_start, self.pos);
        let name_text = name.text(self.text).to_string();

        let mut attributes = Vec::new();
        let mut element_expressions = Vec::new();
        let mut self_closing = false;
        loop {
            self.skip_whitespace();
            if self.pos >= self.bytes.len() {
                break;
            }
            if let Some(ph) = self.placeholder_at(self.pos) {
                self.pos = ph.end;
                element_expressions.push(PlaceholderRef {
                    index: ph.index,
                    span: Span::new(ph.start, ph.end),
                });
                continue;
            }
            let b = self.byte(self.pos);
            if b == b'>' {
                self.pos += 1;
                break;
            }
            if b == b'/' && self.byte(self.pos + 1) == b'>' {
                self_closing = true;
                self.pos += 2;
                break;
            }
            if b == b'/' {
                self.pos += 1;
                continue;
            }
            if b == b'<' {
                // A new tag opening inside this one: the open tag was never
                // finished. Stop here and let the outer loop see it.
                break;
            }
            attributes.push(self.parse_attribute());
        }
        let open = Span::new(tag_start, self.pos);

        // Implied end tags: `<li>` before `<li>`, a block before an open `<p>`.
        while let Some(top) = self.stack.last() {
            let top_name = top.name.text(self.text);
            if !self.in_foreign_content() && implies_end(top_name, &name_text) {
                let mut el = self.stack.pop().unwrap();
                el.closed_implicitly = true;
                self.push_node(Node::Element(el));
            } else {
                break;
            }
        }

        let kind = self.classify(&name_text);
        let void = !matches!(kind, ElementKind::Svg | ElementKind::MathMl) && is_void(&name_text);

        let mut element = Element {
            name,
            open,
            close: None,
            self_closing,
            kind,
            is_void: void,
            attributes,
            element_expressions,
            children: Vec::new(),
            closed_implicitly: false,
        };

        if self_closing {
            let foreign = matches!(kind, ElementKind::Svg | ElementKind::MathMl);
            if !foreign && !void {
                self.errors.push(ParseError {
                    kind: ParseErrorKind::SelfClosedNonVoid,
                    span: name,
                    tag: name_text,
                });
            }
            self.push_node(Node::Element(element));
            return;
        }
        if void {
            self.push_node(Node::Element(element));
            return;
        }
        if !self.in_foreign_content()
            && RAW_TEXT_ELEMENTS
                .iter()
                .any(|r| name_text.eq_ignore_ascii_case(r))
        {
            self.parse_raw_text(&mut element, &name_text);
            self.push_node(Node::Element(element));
            return;
        }
        if matches!(kind, ElementKind::Svg | ElementKind::MathMl)
            && (name_text.eq_ignore_ascii_case("svg") || name_text.eq_ignore_ascii_case("math"))
        {
            self.foreign_depth += 1;
        }
        self.stack.push(element);
    }

    fn classify(&self, name: &str) -> ElementKind {
        if self.in_foreign_content() {
            // Inherit the nearest foreign root's flavour.
            for el in self.stack.iter().rev() {
                match el.kind {
                    ElementKind::Svg => return ElementKind::Svg,
                    ElementKind::MathMl => return ElementKind::MathMl,
                    _ => continue,
                }
            }
        }
        if name.eq_ignore_ascii_case("svg") {
            return ElementKind::Svg;
        }
        if name.eq_ignore_ascii_case("math") {
            return ElementKind::MathMl;
        }
        if name.contains('-') {
            return ElementKind::Custom;
        }
        ElementKind::Html
    }

    fn parse_attribute(&mut self) -> Attribute {
        let full_start = self.pos;
        let modifier = match self.byte(self.pos) {
            b':' => Some(Modifier::Property),
            b'?' => Some(Modifier::Boolean),
            b'@' => Some(Modifier::Event),
            _ => None,
        }
        .map(|m| {
            let span = Span::new(self.pos, self.pos + 1);
            self.pos += 1;
            (m, span)
        });

        let name_start = self.pos;
        let limit = self.next_placeholder_start(self.pos);
        while self.pos < limit && Self::is_name_byte(self.byte(self.pos)) {
            self.pos += 1;
        }
        if self.pos == name_start && self.pos < self.bytes.len() && limit != self.pos {
            // Not a name character at all — swallow one byte so the attribute
            // loop cannot spin.
            self.pos += 1;
        }
        let name = Span::new(name_start, self.pos);

        let mut value = None;
        let after_name = self.pos;
        self.skip_whitespace();
        if self.byte(self.pos) == b'=' {
            self.pos += 1;
            self.skip_whitespace();
            value = Some(self.parse_attribute_value());
        } else {
            self.pos = after_name;
        }
        let full_end = value
            .as_ref()
            .map(|v: &AttributeValue| match v.quote {
                Some(_) => v.span.end + 1,
                None => v.span.end,
            })
            .unwrap_or(self.pos);
        Attribute {
            modifier,
            name,
            full: Span::new(full_start, full_end),
            value,
        }
    }

    fn parse_attribute_value(&mut self) -> AttributeValue {
        let quote = match self.byte(self.pos) {
            b'"' => Some(Quote::Double),
            b'\'' => Some(Quote::Single),
            _ => None,
        };
        if let Some(q) = quote {
            let quote_byte = match q {
                Quote::Double => b'"',
                Quote::Single => b'\'',
            };
            self.pos += 1;
            let value_start = self.pos;
            let mut parts = Vec::new();
            let mut literal_start = self.pos;
            while self.pos < self.bytes.len() && self.byte(self.pos) != quote_byte {
                if let Some(ph) = self.placeholder_at(self.pos) {
                    if literal_start < self.pos {
                        parts.push(AttrPart::Literal(Span::new(literal_start, self.pos)));
                    }
                    parts.push(AttrPart::Placeholder(PlaceholderRef {
                        index: ph.index,
                        span: Span::new(ph.start, ph.end),
                    }));
                    self.pos = ph.end;
                    literal_start = self.pos;
                } else {
                    self.pos += 1;
                }
            }
            if literal_start < self.pos {
                parts.push(AttrPart::Literal(Span::new(literal_start, self.pos)));
            }
            let span = Span::new(value_start, self.pos);
            if self.pos < self.bytes.len() {
                self.pos += 1; // closing quote
            }
            return AttributeValue { quote, span, parts };
        }

        // Unquoted value: up to whitespace, `>`, or `/>`.
        let value_start = self.pos;
        let mut parts = Vec::new();
        let mut literal_start = self.pos;
        while self.pos < self.bytes.len() {
            if let Some(ph) = self.placeholder_at(self.pos) {
                if literal_start < self.pos {
                    parts.push(AttrPart::Literal(Span::new(literal_start, self.pos)));
                }
                parts.push(AttrPart::Placeholder(PlaceholderRef {
                    index: ph.index,
                    span: Span::new(ph.start, ph.end),
                }));
                self.pos = ph.end;
                literal_start = self.pos;
                continue;
            }
            let b = self.byte(self.pos);
            if b.is_ascii_whitespace() || b == b'>' || (b == b'/' && self.byte(self.pos + 1) == b'>')
            {
                break;
            }
            self.pos += 1;
        }
        if literal_start < self.pos {
            parts.push(AttrPart::Literal(Span::new(literal_start, self.pos)));
        }
        AttributeValue {
            quote: None,
            span: Span::new(value_start, self.pos),
            parts,
        }
    }

    // ------------------------------------------------------------- raw text

    fn parse_raw_text(&mut self, element: &mut Element, name: &str) {
        let close_pattern_len = name.len() + 2; // </name
        let mut literal_start = self.pos;
        loop {
            if self.pos >= self.bytes.len() {
                if literal_start < self.pos {
                    element
                        .children
                        .push(Node::Text(Span::new(literal_start, self.pos)));
                }
                self.errors.push(ParseError {
                    kind: ParseErrorKind::UnclosedTag,
                    span: element.name,
                    tag: name.to_string(),
                });
                return;
            }
            if let Some(ph) = self.placeholder_at(self.pos) {
                if literal_start < self.pos {
                    element
                        .children
                        .push(Node::Text(Span::new(literal_start, self.pos)));
                }
                element.children.push(Node::Placeholder(PlaceholderRef {
                    index: ph.index,
                    span: Span::new(ph.start, ph.end),
                }));
                self.pos = ph.end;
                literal_start = self.pos;
                continue;
            }
            if self.byte(self.pos) == b'<'
                && self.starts_with_ci(self.pos + 1, "/")
                && self.starts_with_ci(self.pos + 2, name)
            {
                let after = self.byte(self.pos + close_pattern_len);
                if after == b'>' || after.is_ascii_whitespace() || after == 0 {
                    break;
                }
            }
            self.pos += 1;
        }
        if literal_start < self.pos {
            element
                .children
                .push(Node::Text(Span::new(literal_start, self.pos)));
        }
        // Consume the close tag.
        let close_start = self.pos;
        self.pos += 2; // </
        let name_start = self.pos;
        while Self::is_name_byte(self.byte(self.pos)) {
            self.pos += 1;
        }
        let close_name = Span::new(name_start, self.pos);
        self.skip_whitespace();
        if self.byte(self.pos) == b'>' {
            self.pos += 1;
        }
        element.close = Some(CloseTag {
            span: Span::new(close_start, self.pos),
            name: close_name,
        });
    }

    // ------------------------------------------------------------ close tags

    fn parse_close_tag(&mut self) {
        let close_start = self.pos;
        self.pos += 2; // </
        let name_start = self.pos;
        while Self::is_name_byte(self.byte(self.pos)) {
            self.pos += 1;
        }
        let close_name = Span::new(name_start, self.pos);
        let name_text = close_name.text(self.text).to_string();
        self.skip_whitespace();
        if self.byte(self.pos) == b'>' {
            self.pos += 1;
        }
        let close = CloseTag {
            span: Span::new(close_start, self.pos),
            name: close_name,
        };

        let matches_at = self
            .stack
            .iter()
            .rposition(|el| el.name.text(self.text).eq_ignore_ascii_case(&name_text));
        let Some(depth) = matches_at else {
            self.errors.push(ParseError {
                kind: ParseErrorKind::StrayCloseTag,
                span: close_name,
                tag: name_text,
            });
            return;
        };

        // Close everything above the match: implied for optional-end elements,
        // an error for the rest.
        while self.stack.len() > depth + 1 {
            let el = self.stack.pop().unwrap();
            self.finish_unclosed(el);
        }
        let mut el = self.stack.pop().unwrap();
        if matches!(el.kind, ElementKind::Svg | ElementKind::MathMl) {
            let root_name = el.name.text(self.text);
            if root_name.eq_ignore_ascii_case("svg") || root_name.eq_ignore_ascii_case("math") {
                self.foreign_depth = self.foreign_depth.saturating_sub(1);
            }
        }
        el.close = Some(close);
        self.push_node(Node::Element(el));
    }

    fn finish_unclosed(&mut self, mut el: Element) {
        let name = el.name.text(self.text).to_string();
        if matches!(el.kind, ElementKind::Svg | ElementKind::MathMl)
            && (name.eq_ignore_ascii_case("svg") || name.eq_ignore_ascii_case("math"))
        {
            self.foreign_depth = self.foreign_depth.saturating_sub(1);
        }
        if has_optional_end(&name) {
            el.closed_implicitly = true;
        } else {
            self.errors.push(ParseError {
                kind: ParseErrorKind::UnclosedTag,
                span: el.name,
                tag: name,
            });
        }
        self.push_node(Node::Element(el));
    }
}

#[cfg(test)]
mod tests;
