use pretty_assertions::assert_eq;

use super::*;

/// Build the placeholder table the way the virtual document does: every
/// `${…}` region in `source` becomes an underscore run of the same length.
/// Test inputs are written with real `${…}` for the eye; this converts them.
fn substitute(source: &str) -> (String, Vec<Placeholder>) {
    let mut out = Vec::with_capacity(source.len());
    let mut placeholders = Vec::new();
    let bytes = source.as_bytes();
    let mut pos = 0;
    let mut index = 0u32;
    while pos < bytes.len() {
        if bytes[pos] == b'$' && pos + 1 < bytes.len() && bytes[pos + 1] == b'{' {
            let start = pos;
            let mut depth = 0usize;
            while pos < bytes.len() {
                match bytes[pos] {
                    b'{' => depth += 1,
                    b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            pos += 1;
                            break;
                        }
                    }
                    _ => {}
                }
                pos += 1;
            }
            placeholders.push(Placeholder {
                index,
                start,
                end: pos,
            });
            index += 1;
            out.resize(pos, b'_');
        } else {
            out.push(bytes[pos]);
            pos += 1;
        }
    }
    (String::from_utf8(out).unwrap(), placeholders)
}

fn parse_source(source: &str) -> (String, Document) {
    let (text, placeholders) = substitute(source);
    let doc = parse(&text, &placeholders);
    (text, doc)
}

fn first_element(doc: &Document) -> &Element {
    doc.children
        .iter()
        .find_map(|n| match n {
            Node::Element(e) => Some(e),
            _ => None,
        })
        .expect("no element parsed")
}

// ----------------------------------------------------------------- tokenizer

#[test]
fn plain_element_with_attributes() {
    let (text, doc) = parse_source(r#"<div class="chrome" role="toolbar">x</div>"#);
    assert_eq!(doc.errors, vec![]);
    let el = first_element(&doc);
    assert_eq!(el.name.text(&text), "div");
    assert_eq!(el.attributes.len(), 2);
    assert_eq!(el.attributes[0].name.text(&text), "class");
    let value = el.attributes[0].value.as_ref().unwrap();
    assert_eq!(value.span.text(&text), "chrome");
    assert_eq!(value.quote, Some(Quote::Double));
    assert_eq!(el.children.len(), 1);
    assert!(matches!(el.children[0], Node::Text(_)));
    assert!(el.close.is_some());
}

#[test]
fn modifier_spans_are_separate_from_names() {
    let (text, doc) =
        parse_source(r#"<input :value="${x => x.q}" ?disabled="${x => x.l}" @input="${h}">"#);
    assert_eq!(doc.errors, vec![]);
    let el = first_element(&doc);
    let mods: Vec<(Modifier, &str)> = el
        .attributes
        .iter()
        .map(|a| (a.modifier.unwrap().0, a.name.text(&text)))
        .collect();
    assert_eq!(
        mods,
        vec![
            (Modifier::Property, "value"),
            (Modifier::Boolean, "disabled"),
            (Modifier::Event, "input"),
        ]
    );
    // The modifier's own span is the single prefix character.
    let (_, span) = el.attributes[0].modifier.unwrap();
    assert_eq!(span.text(&text), ":");
    // Each value is exactly one placeholder.
    for attr in &el.attributes {
        let v = attr.value.as_ref().unwrap();
        assert!(v.single_placeholder().is_some(), "{}", attr.name.text(&text));
    }
}

#[test]
fn mixed_attribute_value_parts() {
    let (text, doc) = parse_source(r#"<button class="btn ${x => (x.on ? 'on' : '')}">b</button>"#);
    let el = first_element(&doc);
    let value = el.attributes[0].value.as_ref().unwrap();
    assert_eq!(value.parts.len(), 2);
    assert!(matches!(value.parts[0], AttrPart::Literal(s) if s.text(&text) == "btn "));
    assert!(matches!(value.parts[1], AttrPart::Placeholder(p) if p.index == 0));
}

#[test]
fn two_placeholders_in_one_value() {
    let (_, doc) = parse_source(r#"<div style="left: ${x => x.a}px; top: ${x => x.b}px">m</div>"#);
    let el = first_element(&doc);
    let value = el.attributes[0].value.as_ref().unwrap();
    let indices: Vec<u32> = value.placeholders().map(|p| p.index).collect();
    assert_eq!(indices, vec![0, 1]);
    // literal, placeholder, literal, placeholder, literal
    assert_eq!(value.parts.len(), 5);
}

#[test]
fn element_expression_between_attributes() {
    let (text, doc) = parse_source(r#"<div class="table" ${ref('tableEl')} tabindex="0">t</div>"#);
    assert_eq!(doc.errors, vec![]);
    let el = first_element(&doc);
    assert_eq!(el.attributes.len(), 2);
    assert_eq!(el.element_expressions.len(), 1);
    assert_eq!(el.element_expressions[0].index, 0);
    assert_eq!(el.attributes[1].name.text(&text), "tabindex");
}

#[test]
fn placeholder_in_content_position() {
    let (_, doc) = parse_source(r#"<span>${x => x.count}</span>"#);
    let el = first_element(&doc);
    assert_eq!(el.children.len(), 1);
    assert!(matches!(el.children[0], Node::Placeholder(p) if p.index == 0));
}

#[test]
fn unquoted_placeholder_value() {
    let (_, doc) = parse_source(r#"<input ?disabled=${x => x.locked}>"#);
    let el = first_element(&doc);
    let value = el.attributes[0].value.as_ref().unwrap();
    assert_eq!(value.quote, None);
    assert!(value.single_placeholder().is_some());
}

#[test]
fn comments_and_doctype() {
    let (text, doc) = parse_source("<!doctype html><!-- a comment --><div></div>");
    assert!(matches!(doc.children[0], Node::Doctype(_)));
    assert!(matches!(doc.children[1], Node::Comment(s) if s.text(&text) == "<!-- a comment -->"));
    assert!(matches!(doc.children[2], Node::Element(_)));
}

#[test]
fn unterminated_comment_runs_to_eof() {
    let (_, doc) = parse_source("<div><!-- oops</div>");
    let el = first_element(&doc);
    // The comment swallows the close tag, so the div is unclosed.
    assert!(el.close.is_none());
    assert!(doc
        .errors
        .iter()
        .any(|e| e.kind == ParseErrorKind::UnclosedTag && e.tag == "div"));
}

// -------------------------------------------------------------- tree builder

#[test]
fn nesting() {
    let (text, doc) = parse_source("<div><span><b>x</b></span></div>");
    assert_eq!(doc.errors, vec![]);
    let div = first_element(&doc);
    let Node::Element(span) = &div.children[0] else {
        panic!()
    };
    let Node::Element(b) = &span.children[0] else {
        panic!()
    };
    assert_eq!(b.name.text(&text), "b");
}

#[test]
fn unclosed_tag_is_reported_not_repaired() {
    let (_, doc) = parse_source("<div><butto>x</div>");
    assert_eq!(doc.errors.len(), 1);
    assert_eq!(doc.errors[0].kind, ParseErrorKind::UnclosedTag);
    assert_eq!(doc.errors[0].tag, "butto");
    // The unclosed element stays where it was written: a child of the div.
    let div = first_element(&doc);
    assert!(div.close.is_some());
    let Node::Element(butto) = &div.children[0] else {
        panic!()
    };
    assert!(butto.close.is_none());
    assert!(!butto.closed_implicitly);
}

#[test]
fn void_elements_do_not_nest() {
    let (_, doc) = parse_source("<div><br><input type=\"text\"><hr></div>");
    assert_eq!(doc.errors, vec![]);
    let div = first_element(&doc);
    assert_eq!(div.children.len(), 3);
    for child in &div.children {
        let Node::Element(el) = child else { panic!() };
        assert!(el.is_void);
        assert!(el.close.is_none());
    }
}

#[test]
fn optional_end_tags_close_implicitly() {
    let (text, doc) = parse_source("<ul><li>a<li>b</ul>");
    assert_eq!(doc.errors, vec![]);
    let ul = first_element(&doc);
    assert_eq!(ul.children.len(), 2);
    for child in &ul.children {
        let Node::Element(li) = child else { panic!() };
        assert_eq!(li.name.text(&text), "li");
        assert!(li.closed_implicitly || li.close.is_some());
    }
}

#[test]
fn p_closed_by_block() {
    let (_, doc) = parse_source("<p>one<div>two</div>");
    assert_eq!(doc.errors, vec![]);
    let p = first_element(&doc);
    assert!(p.closed_implicitly);
    // The div is a sibling, not a child of the p.
    assert_eq!(doc.children.len(), 2);
}

#[test]
fn self_closing_html_element_is_an_error() {
    let (_, doc) = parse_source("<div/><my-el/>");
    let kinds: Vec<ParseErrorKind> = doc.errors.iter().map(|e| e.kind).collect();
    assert_eq!(
        kinds,
        vec![
            ParseErrorKind::SelfClosedNonVoid,
            ParseErrorKind::SelfClosedNonVoid
        ]
    );
}

#[test]
fn svg_self_closing_is_legal() {
    let (text, doc) = parse_source(
        r#"<svg viewBox="0 0 16 16"><path d="M6 3L2 8l4 5" fill="none"/><circle cx="7" cy="7" r="4"/></svg>"#,
    );
    assert_eq!(doc.errors, vec![]);
    let svg = first_element(&doc);
    assert_eq!(svg.kind, ElementKind::Svg);
    assert_eq!(svg.children.len(), 2);
    for child in &svg.children {
        let Node::Element(el) = child else { panic!() };
        assert_eq!(el.kind, ElementKind::Svg, "{}", el.name.text(&text));
        assert!(el.self_closing);
    }
}

#[test]
fn foreign_content_ends_with_the_svg() {
    let (_, doc) = parse_source("<svg><rect/></svg><span/>");
    // The span is outside the svg: self-closing is an error again.
    assert_eq!(doc.errors.len(), 1);
    assert_eq!(doc.errors[0].kind, ParseErrorKind::SelfClosedNonVoid);
    assert_eq!(doc.errors[0].tag, "span");
}

#[test]
fn custom_element_kind() {
    let (_, doc) = parse_source("<csv-grid></csv-grid>");
    assert_eq!(first_element(&doc).kind, ElementKind::Custom);
}

#[test]
fn raw_text_swallows_markup() {
    let (text, doc) = parse_source("<style>.a > .b { color: red; }</style><div></div>");
    assert_eq!(doc.errors, vec![]);
    let style = first_element(&doc);
    assert_eq!(style.children.len(), 1);
    let Node::Text(s) = &style.children[0] else {
        panic!()
    };
    assert_eq!(s.text(&text), ".a > .b { color: red; }");
    assert!(style.close.is_some());
}

#[test]
fn raw_text_with_placeholder() {
    let (_, doc) = parse_source("<style>${sheet}</style>");
    let style = first_element(&doc);
    assert_eq!(style.children.len(), 1);
    assert!(matches!(style.children[0], Node::Placeholder(p) if p.index == 0));
}

#[test]
fn textarea_and_title_are_raw_text() {
    let (text, doc) = parse_source("<textarea><div>not a div</div></textarea>");
    let ta = first_element(&doc);
    assert_eq!(doc.errors, vec![]);
    let Node::Text(s) = &ta.children[0] else {
        panic!()
    };
    assert_eq!(s.text(&text), "<div>not a div</div>");
}

#[test]
fn title_inside_svg_is_not_raw_text() {
    let (_, doc) = parse_source("<svg><title><tspan>t</tspan></title></svg>");
    assert_eq!(doc.errors, vec![]);
    let svg = first_element(&doc);
    let Node::Element(title) = &svg.children[0] else {
        panic!()
    };
    assert!(matches!(&title.children[0], Node::Element(_)));
}

#[test]
fn stray_close_tag() {
    let (_, doc) = parse_source("<div></span></div>");
    assert_eq!(doc.errors.len(), 1);
    assert_eq!(doc.errors[0].kind, ParseErrorKind::StrayCloseTag);
    assert_eq!(doc.errors[0].tag, "span");
    assert!(first_element(&doc).close.is_some());
}

#[test]
fn attribute_value_with_angle_brackets() {
    let (text, doc) = parse_source(r#"<path d="M6 3L2 8l4 5" data-x="a > b < c"></path>"#);
    assert_eq!(doc.errors, vec![]);
    let el = first_element(&doc);
    assert_eq!(
        el.attributes[1].value.as_ref().unwrap().span.text(&text),
        "a > b < c"
    );
}

#[test]
fn close_tag_with_whitespace() {
    let (_, doc) = parse_source("<div></div >");
    assert_eq!(doc.errors, vec![]);
    assert!(first_element(&doc).close.is_some());
}

#[test]
fn spans_index_the_input_exactly() {
    let source = r#"<a href="x">${t}</a>"#;
    let (text, doc) = parse_source(source);
    let a = first_element(&doc);
    assert_eq!(a.open, Span::new(0, 12));
    assert_eq!(a.name, Span::new(1, 2));
    assert_eq!(a.attributes[0].name, Span::new(3, 7));
    assert_eq!(a.attributes[0].value.as_ref().unwrap().span, Span::new(9, 10));
    assert_eq!(a.children[0].span(), Span::new(12, 16));
    assert_eq!(a.close.unwrap().span, Span::new(16, 20));
    assert_eq!(&text[12..16], "____");
}

#[test]
fn nested_when_shape_from_the_corpus() {
    // The outer template as the substitution leaves it: the inner html`…` is
    // a separate document, so here it is just part of the expression run.
    let source = "<span class=\"chip\">${when((x) => !x.readOnly, html_inner)}</span>";
    let (_, doc) = parse_source(source);
    assert_eq!(doc.errors, vec![]);
    let el = first_element(&doc);
    assert_eq!(el.children.len(), 1);
    assert!(matches!(el.children[0], Node::Placeholder(_)));
}

#[test]
fn deeply_wrong_nesting_recovers() {
    let (_, doc) = parse_source("<div><span></div>");
    // span is unclosed (reported); div closes.
    assert_eq!(doc.errors.len(), 1);
    assert_eq!(doc.errors[0].tag, "span");
    assert!(first_element(&doc).close.is_some());
}

#[test]
fn empty_input() {
    let doc = parse("", &[]);
    assert_eq!(doc.children, vec![]);
    assert_eq!(doc.errors, vec![]);
}

#[test]
fn text_only() {
    let (text, doc) = parse_source("just words, no markup");
    assert_eq!(doc.children.len(), 1);
    assert!(matches!(doc.children[0], Node::Text(s) if s.text(&text) == "just words, no markup"));
}

#[test]
fn non_ascii_text_keeps_byte_spans_consistent() {
    let source = "<button title=\"↑ · ⌘\">✕</button>";
    let (text, doc) = parse_source(source);
    assert_eq!(doc.errors, vec![]);
    let el = first_element(&doc);
    let value = el.attributes[0].value.as_ref().unwrap();
    assert_eq!(value.span.text(&text), "↑ · ⌘");
    let Node::Text(t) = &el.children[0] else {
        panic!()
    };
    assert_eq!(t.text(&text), "✕");
}

// ---------------------------------------------------------- multibyte input

#[test]
fn attribute_starting_with_a_multibyte_char() {
    let (text, doc) = parse_source("<a é>");
    let el = first_element(&doc);
    assert_eq!(el.attributes[0].name.text(&text), "é");
}

#[test]
fn markup_declaration_followed_by_a_multibyte_char() {
    // `<!` then a check for `<!--` / `<!doctype` that reaches into the `中`.
    parse_source("<!中");
    parse_source("<div></中");
}
