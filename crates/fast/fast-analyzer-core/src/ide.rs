//! Position-based IDE features: completion, quick info, definition,
//! references, rename, closing tags, folding, code fixes — all of them
//! "resolve a position in the tree, then look something up".
//!
//! Spans in: UTF-16 document offsets. Spans out: UTF-16, document-relative
//! for the current document, absolute `FileSpan`s for anything cross-file.

use fast_template_syntax::{Attribute, Element, ElementKind, Modifier, Node, Span};

use crate::config::resolved_severities;
use crate::documents::DocumentState;
use crate::protocol::{
    ClosingTagResult, CompletionItem, Completions, ComponentFact, DefinitionResult,
    DocumentInfoAt, FileDiagnostic, FileSpan, FoldingRange, Query, QuickInfo, RenameInfo,
    TreeNode,
};
use crate::registry::TagKnowledge;
use crate::Engine;

/// What the cursor is on.
enum Position<'a> {
    TagName { el: &'a Element, closing: bool },
    AttrName { el: &'a Element, attr: &'a Attribute },
    AttrValue { el: &'a Element, attr: &'a Attribute, parent: Option<&'a Element> },
    ElementExpression,
    ContentPlaceholder,
    InOpenTag { el: &'a Element },
    Content,
}

fn resolve_position<'a>(nodes: &'a [Node], offset: usize, parent: Option<&'a Element>) -> Position<'a> {
    for node in nodes {
        match node {
            Node::Element(el) => {
                if el.open.contains(offset) || el.open.end == offset {
                    if el.name.touches(offset) {
                        return Position::TagName { el, closing: false };
                    }
                    for attr in &el.attributes {
                        if attr.name.touches(offset)
                            || attr
                                .modifier
                                .map(|(_, s)| s.touches(offset))
                                .unwrap_or(false)
                        {
                            return Position::AttrName { el, attr };
                        }
                        if let Some(value) = &attr.value {
                            if value.span.touches(offset) {
                                return Position::AttrValue { el, attr, parent };
                            }
                        }
                    }
                    for ph in &el.element_expressions {
                        if ph.span.contains(offset) {
                            return Position::ElementExpression;
                        }
                    }
                    if el.open.contains(offset) {
                        return Position::InOpenTag { el };
                    }
                }
                if let Some(close) = &el.close {
                    if close.name.touches(offset) && close.span.contains(offset) {
                        return Position::TagName { el, closing: true };
                    }
                }
                let span = el.span();
                if span.contains(offset) {
                    return resolve_position(&el.children, offset, Some(el));
                }
            }
            Node::Placeholder(ph) => {
                if ph.span.contains(offset) {
                    return Position::ContentPlaceholder;
                }
            }
            Node::Text(_) | Node::Comment(_) | Node::Doctype(_) => {}
        }
    }
    let _ = parent;
    Position::Content
}

impl Engine {
    pub fn query(&self, query: Query) -> serde_json::Value {
        match query {
            Query::Completions { document_id, offset } => {
                json(self.completions(&document_id, offset))
            }
            Query::QuickInfo { document_id, offset } => {
                json(self.quick_info(&document_id, offset))
            }
            Query::Definition { document_id, offset } => {
                json(self.definition(&document_id, offset))
            }
            Query::References { document_id, offset } => {
                json(self.references_at(&document_id, offset))
            }
            Query::MemberReferences { tag, source_type_id, name } => {
                json(Some(self.member_locations(tag.as_deref(), source_type_id, &name)))
            }
            Query::TagReferences { tag } => json(Some(self.tag_locations(&tag, false))),
            Query::RenameInfo { document_id, offset } => {
                json(self.rename_info(&document_id, offset))
            }
            Query::RenameLocations { document_id, offset } => {
                json(self.rename_locations_at(&document_id, offset))
            }
            Query::MemberRenameLocations { tag, source_type_id, name } => {
                json(Some(self.member_locations(tag.as_deref(), source_type_id, &name)))
            }
            Query::TagRenameLocations { tag } => json(Some(self.tag_locations(&tag, true))),
            Query::ClosingTag { document_id, offset } => {
                json(self.closing_tag(&document_id, offset))
            }
            Query::Folding { document_id } => json(self.folding(&document_id)),
            Query::CodeFixes { document_id, start, end } => {
                json(self.code_fixes(&document_id, start, end))
            }
            Query::DocumentInfoAt { document_id, offset } => {
                json(self.document_info_at(&document_id, offset))
            }
            Query::Severities => json(Some(resolved_severities(&self.config))),
            Query::FileDiagnostics { file_name } => {
                json(Some(self.file_diagnostics(&file_name)))
            }
            Query::ParseTree { document_id } => json(self.parse_tree(&document_id)),
        }
    }

    fn parse_tree(&self, document_id: &str) -> Option<Vec<TreeNode>> {
        let doc = self.document(document_id)?;
        let tree = doc.tree.as_ref()?;
        fn convert(nodes: &[Node], doc: &DocumentState) -> Vec<TreeNode> {
            let text = doc.text();
            nodes
                .iter()
                .filter_map(|node| match node {
                    Node::Element(el) => Some(TreeNode {
                        kind: "element".into(),
                        name: el.name.text(text).to_ascii_lowercase(),
                        attrs: {
                            let mut attrs: Vec<String> = el
                                .attributes
                                .iter()
                                .map(|a| {
                                    let modifier = a
                                        .modifier
                                        .map(|(m, _)| m.char().to_string())
                                        .unwrap_or_default();
                                    format!("{modifier}{}", a.name.text(text).to_ascii_lowercase())
                                })
                                .collect();
                            attrs.sort();
                            attrs
                        },
                        element_expressions: el.element_expressions.len() as u32,
                        closed: el.close.is_some(),
                        implied: el.closed_implicitly,
                        self_closing: el.self_closing,
                        children: convert(&el.children, doc),
                    }),
                    Node::Text(span) => {
                        let content = span.text(text).trim().to_string();
                        (!content.is_empty()).then(|| TreeNode {
                            kind: "text".into(),
                            name: content,
                            ..TreeNode::default()
                        })
                    }
                    Node::Placeholder(_) => Some(TreeNode {
                        kind: "placeholder".into(),
                        ..TreeNode::default()
                    }),
                    Node::Comment(_) | Node::Doctype(_) => None,
                })
                .collect()
        }
        Some(convert(&tree.children, doc))
    }

    // ---------------------------------------------------------- completions

    fn completions(&self, document_id: &str, offset: u32) -> Option<Completions> {
        let doc = self.document(document_id)?;
        let tree = doc.tree.as_ref()?;
        let byte = doc.byte_of_utf16(offset);
        let text = doc.text();

        // `<` or `</` immediately before the cursor beats tree positions: the
        // tag being typed usually parses as text or a broken element.
        let before = &text[..byte.min(text.len())];
        if before.ends_with("</") {
            return Some(self.closing_name_completions(doc, byte));
        }

        match resolve_position(&tree.children, byte, None) {
            Position::TagName { el, closing: false } => {
                let (start, end) = doc.utf16_span(el.name);
                let mut completions = self.tag_completions(doc, el.kind);
                completions.replace_start = Some(start);
                completions.replace_end = Some(end.max(offset));
                Some(completions)
            }
            Position::TagName { el, closing: true } => {
                let name = el.name.text(text).to_string();
                let close_name = el.close.as_ref().map(|c| c.name).unwrap_or(el.name);
                let (start, end) = doc.utf16_span(close_name);
                Some(Completions {
                    items: vec![CompletionItem {
                        name,
                        kind: "tag".into(),
                        ..CompletionItem::default()
                    }],
                    replace_start: Some(start),
                    replace_end: Some(end.max(offset)),
                })
            }
            Position::AttrName { el, attr } => {
                let modifier = attr.modifier.map(|(m, _)| m);
                let full_start = attr
                    .modifier
                    .map(|(_, s)| s.start)
                    .unwrap_or(attr.name.start);
                let (start, _) = doc.utf16_span(Span::new(full_start, full_start));
                let (_, end) = doc.utf16_span(attr.name);
                let mut completions = self.attribute_completions(doc, el, modifier);
                completions.replace_start = Some(start);
                completions.replace_end = Some(end.max(offset));
                Some(completions)
            }
            Position::AttrValue { el, attr, parent } => {
                let (start, end) = doc.utf16_span(attr.value.as_ref().unwrap().span);
                let mut completions = self.value_completions(doc, el, attr, parent);
                completions.replace_start = Some(start);
                completions.replace_end = Some(end.max(offset));
                Some(completions)
            }
            Position::InOpenTag { el } => Some(self.attribute_completions(doc, el, None)),
            Position::Content => {
                if before.ends_with('<') {
                    let kind = enclosing_kind(&tree.children, byte);
                    return Some(self.tag_completions(doc, kind));
                }
                Some(self.snippet_completions())
            }
            Position::ElementExpression | Position::ContentPlaceholder => None,
        }
    }

    fn closing_name_completions(&self, doc: &DocumentState, byte: usize) -> Completions {
        // The nearest enclosing unclosed element is what `</` wants to close.
        let Some(tree) = doc.tree.as_ref() else {
            return Completions::default();
        };
        let mut open: Vec<String> = Vec::new();
        collect_unclosed(&tree.children, byte, doc.text(), &mut open);
        Completions {
            items: open
                .into_iter()
                .rev()
                .map(|name| CompletionItem {
                    insert_text: Some(format!("{name}>")),
                    name,
                    kind: "tag".into(),
                    ..CompletionItem::default()
                })
                .collect(),
            replace_start: None,
            replace_end: None,
        }
    }

    fn tag_completions(&self, doc: &DocumentState, kind: ElementKind) -> Completions {
        let mut items = Vec::new();
        for tag in self.registry.known_custom_tags(&self.config) {
            let comps = self.registry.components_for_tag(&tag);
            let documentation = comps
                .first()
                .and_then(|c| c.documentation.clone())
                .or_else(|| {
                    self.registry
                        .custom_tag(&tag)
                        .map(|t| t.description.clone())
                });
            let import_from = if comps.is_empty()
                || self
                    .registry
                    .is_reachable(&doc.fact.file_name, &tag, self.config.max_project_import_depth)
            {
                None
            } else {
                self.registry
                    .declaring_files(&tag)
                    .first()
                    .map(|f| f.to_string())
            };
            items.push(CompletionItem {
                name: tag,
                kind: "tag".into(),
                sort_text: Some("0".into()),
                documentation,
                import_from,
                ..CompletionItem::default()
            });
        }
        let namespace = match kind {
            ElementKind::Svg => fast_html_data::Namespace::Svg,
            ElementKind::MathMl => fast_html_data::Namespace::MathMl,
            _ => fast_html_data::Namespace::Html,
        };
        for el in fast_html_data::elements().filter(|e| e.namespace == namespace) {
            items.push(CompletionItem {
                name: el.name.to_string(),
                kind: "tag".into(),
                sort_text: Some("1".into()),
                documentation: Some(el.description.to_string()),
                ..CompletionItem::default()
            });
        }
        Completions {
            items,
            replace_start: None,
            replace_end: None,
        }
    }

    fn attribute_completions(
        &self,
        doc: &DocumentState,
        el: &Element,
        modifier: Option<Modifier>,
    ) -> Completions {
        let text = doc.text();
        let tag = el.name.text(text);
        let knowledge = self.registry.lookup(tag, el.kind, &self.config);
        let present: Vec<&str> = el.attributes.iter().map(|a| a.name.text(text)).collect();
        let mut items: Vec<CompletionItem> = Vec::new();

        let prefix = |m: Option<Modifier>| -> &'static str {
            // When the user already typed the modifier, complete bare names.
            if modifier.is_some() {
                return "";
            }
            match m {
                Some(Modifier::Property) => ":",
                Some(Modifier::Boolean) => "?",
                Some(Modifier::Event) => "@",
                None => "",
            }
        };

        let wants = |m: Option<Modifier>| modifier.is_none() || modifier == m;

        if wants(None) {
            let mut push_attr = |name: &str, doc_text: Option<String>, type_text: Option<String>, sort: &str| {
                if present.contains(&name) {
                    return;
                }
                items.push(CompletionItem {
                    name: name.to_string(),
                    kind: "attribute".into(),
                    sort_text: Some(sort.to_string()),
                    documentation: doc_text,
                    type_text,
                    ..CompletionItem::default()
                });
            };
            match &knowledge {
                TagKnowledge::Components(comps) => {
                    for a in comps.iter().flat_map(|c| c.attributes.iter()) {
                        push_attr(&a.name, a.documentation.clone(), a.type_text.clone(), "0");
                    }
                }
                TagKnowledge::Custom(custom) => {
                    for a in &custom.attributes {
                        push_attr(&a.name, Some(a.description.clone()), None, "0");
                    }
                }
                TagKnowledge::Builtin(data) => {
                    for a in data.attributes {
                        push_attr(a.name, Some(a.description.to_string()), None, "1");
                    }
                    if matches!(el.kind, ElementKind::Svg | ElementKind::MathMl) {
                        for a in fast_html_data::SVG_PRESENTATION_ATTRIBUTES {
                            push_attr(a.name, Some(a.description.to_string()), None, "2");
                        }
                    }
                }
                _ => {}
            }
            if !matches!(el.kind, ElementKind::Svg | ElementKind::MathMl) {
                for a in fast_html_data::GLOBAL_ATTRIBUTES {
                    push_attr(a.name, Some(a.description.to_string()), None, "3");
                }
            } else {
                for name in ["id", "class", "style", "tabindex", "role", "slot", "part"] {
                    push_attr(name, None, None, "3");
                }
            }
        }

        if wants(Some(Modifier::Property)) {
            let p = prefix(Some(Modifier::Property));
            match &knowledge {
                TagKnowledge::Components(comps) => {
                    for m in comps.iter().flat_map(|c| c.properties.iter()) {
                        items.push(CompletionItem {
                            name: format!("{p}{}", m.name),
                            kind: "property".into(),
                            sort_text: Some("0".into()),
                            documentation: m.documentation.clone(),
                            type_text: m.type_text.clone(),
                            insert_text: Some(format!("{p}{}=\"${{}}\"", m.name)),
                            is_snippet: false,
                            ..CompletionItem::default()
                        });
                    }
                    for a in comps.iter().flat_map(|c| c.attributes.iter()) {
                        if let Some(prop) = &a.property_name {
                            items.push(CompletionItem {
                                name: format!("{p}{prop}"),
                                kind: "property".into(),
                                sort_text: Some("0".into()),
                                documentation: a.documentation.clone(),
                                type_text: a.type_text.clone(),
                                ..CompletionItem::default()
                            });
                        }
                    }
                }
                TagKnowledge::Builtin(_) => {
                    for name in super::rules_dom_extra_properties() {
                        items.push(CompletionItem {
                            name: format!("{p}{name}"),
                            kind: "property".into(),
                            sort_text: Some("2".into()),
                            ..CompletionItem::default()
                        });
                    }
                }
                _ => {}
            }
        }

        if wants(Some(Modifier::Boolean)) {
            let p = prefix(Some(Modifier::Boolean));
            match &knowledge {
                TagKnowledge::Components(comps) => {
                    for a in comps.iter().flat_map(|c| c.attributes.iter()) {
                        if a.mode.as_deref() == Some("boolean")
                            || a.type_text.as_deref() == Some("boolean")
                        {
                            items.push(CompletionItem {
                                name: format!("{p}{}", a.name),
                                kind: "booleanAttribute".into(),
                                sort_text: Some("0".into()),
                                documentation: a.documentation.clone(),
                                ..CompletionItem::default()
                            });
                        }
                    }
                }
                TagKnowledge::Builtin(data) => {
                    for a in data.attributes.iter().filter(|a| a.boolean) {
                        items.push(CompletionItem {
                            name: format!("{p}{}", a.name),
                            kind: "booleanAttribute".into(),
                            sort_text: Some("1".into()),
                            documentation: Some(a.description.to_string()),
                            ..CompletionItem::default()
                        });
                    }
                }
                _ => {}
            }
        }

        if wants(Some(Modifier::Event)) {
            let p = prefix(Some(Modifier::Event));
            if let TagKnowledge::Components(comps) = &knowledge {
                for e in comps.iter().flat_map(|c| c.events.iter()) {
                    items.push(CompletionItem {
                        name: format!("{p}{}", e.name),
                        kind: "event".into(),
                        sort_text: Some("0".into()),
                        documentation: e.documentation.clone(),
                        type_text: e.type_text.clone(),
                        ..CompletionItem::default()
                    });
                }
            }
            if let TagKnowledge::Custom(custom) = &knowledge {
                for e in &custom.events {
                    items.push(CompletionItem {
                        name: format!("{p}{}", e.name),
                        kind: "event".into(),
                        sort_text: Some("0".into()),
                        documentation: Some(e.description.clone()),
                        ..CompletionItem::default()
                    });
                }
            }
            // The project's `HTMLElementEventMap` augmentation: below this
            // tag's own events, above the DOM's — and never twice, when the
            // augmentation restates a name one of those already carries.
            for e in self.registry.global_events() {
                let offered = format!("{p}{}", e.name);
                if fast_html_data::event(&e.name).is_some()
                    || items.iter().any(|i| i.name == offered)
                {
                    continue;
                }
                items.push(CompletionItem {
                    name: offered,
                    kind: "event".into(),
                    sort_text: Some("1".into()),
                    documentation: e.documentation.clone(),
                    type_text: e.type_text.clone(),
                    ..CompletionItem::default()
                });
            }
            for (name, description) in fast_html_data::events() {
                items.push(CompletionItem {
                    name: format!("{p}{name}"),
                    kind: "event".into(),
                    sort_text: Some("2".into()),
                    documentation: Some(description.to_string()),
                    ..CompletionItem::default()
                });
            }
        }

        let _ = doc;
        Completions {
            items,
            replace_start: None,
            replace_end: None,
        }
    }

    fn value_completions(
        &self,
        doc: &DocumentState,
        el: &Element,
        attr: &Attribute,
        parent: Option<&Element>,
    ) -> Completions {
        let text = doc.text();
        let name = attr.name.text(text);
        let tag = el.name.text(text);
        let mut items = Vec::new();

        if attr.modifier.is_none() && name == "slot" {
            if let Some(parent) = parent {
                let parent_tag = parent.name.text(text);
                for comp in self.registry.components_for_tag(parent_tag) {
                    for slot in &comp.slots {
                        if !slot.name.is_empty() {
                            items.push(CompletionItem {
                                name: slot.name.clone(),
                                kind: "slotName".into(),
                                documentation: slot.documentation.clone(),
                                ..CompletionItem::default()
                            });
                        }
                    }
                }
            }
        } else if attr.modifier.is_none() && (name == "part" || name == "exportparts") {
            if let Some(component_tag) = doc.fact.component_tag.as_deref() {
                for comp in self.registry.components_for_tag(component_tag) {
                    for part in &comp.css_parts {
                        items.push(CompletionItem {
                            name: part.name.clone(),
                            kind: "part".into(),
                            documentation: part.documentation.clone(),
                            ..CompletionItem::default()
                        });
                    }
                }
            }
        } else if attr.modifier.is_none() {
            let knowledge = self.registry.lookup(tag, el.kind, &self.config);
            match &knowledge {
                TagKnowledge::Builtin(data) => {
                    if let Some(a) = data.attributes.iter().find(|a| a.name == name) {
                        for value in a.values {
                            items.push(CompletionItem {
                                name: value.to_string(),
                                kind: "value".into(),
                                ..CompletionItem::default()
                            });
                        }
                    } else if let Some(a) = fast_html_data::global_attribute(name) {
                        for value in a.values {
                            items.push(CompletionItem {
                                name: value.to_string(),
                                kind: "value".into(),
                                ..CompletionItem::default()
                            });
                        }
                    }
                }
                TagKnowledge::Components(comps) => {
                    if let Some(member) = self.registry.find_attribute(comps, name) {
                        for value in &member.values {
                            items.push(CompletionItem {
                                name: value.clone(),
                                kind: "value".into(),
                                ..CompletionItem::default()
                            });
                        }
                    }
                }
                _ => {}
            }
        }

        Completions {
            items,
            replace_start: None,
            replace_end: None,
        }
    }

    fn snippet_completions(&self) -> Completions {
        let snippet = |name: &str, insert: &str, doc: &str| CompletionItem {
            name: name.to_string(),
            kind: "snippet".into(),
            insert_text: Some(insert.to_string()),
            documentation: Some(doc.to_string()),
            is_snippet: true,
            ..CompletionItem::default()
        };
        Completions {
            items: vec![
                snippet(
                    "when",
                    "${when((x) => ${1:condition}, html`$2`)}",
                    "Render a template when a condition holds.",
                ),
                snippet(
                    "repeat",
                    "${repeat((x) => ${1:items}, html<${2:Item}, ${3:Parent}>`$4`)}",
                    "Render a template per item. The item becomes the inner template's source type.",
                ),
                snippet(
                    "render",
                    "${render((x) => ${1:value}, ${2:template})}",
                    "Render a value with a template.",
                ),
                snippet(
                    "binding",
                    "${(x) => ${1:x.}}",
                    "A reactive binding.",
                ),
            ],
            replace_start: None,
            replace_end: None,
        }
    }

    // ------------------------------------------------------------ hover

    fn quick_info(&self, document_id: &str, offset: u32) -> Option<QuickInfo> {
        let doc = self.document(document_id)?;
        let tree = doc.tree.as_ref()?;
        let byte = doc.byte_of_utf16(offset);
        let text = doc.text();

        if let Some(info) = self.directive_arg_hover(doc, offset) {
            return Some(info);
        }

        match resolve_position(&tree.children, byte, None) {
            Position::TagName { el, closing } => {
                let tag = el.name.text(text);
                let knowledge = self.registry.lookup(tag, el.kind, &self.config);
                let contents = match &knowledge {
                    TagKnowledge::Components(comps) => {
                        let comp = comps.first()?;
                        component_hover(tag, comp)
                    }
                    TagKnowledge::Builtin(data) => {
                        format!("```html\n<{tag}>\n```\n\n{}", data.description)
                    }
                    TagKnowledge::Custom(custom) => {
                        format!("```html\n<{tag}>\n```\n\n{}", custom.description)
                    }
                    TagKnowledge::GlobalTag => {
                        format!("```html\n<{tag}>\n```\n\nDeclared in `fastElementUltra.globalTags`; nothing about it is checked.")
                    }
                    TagKnowledge::Unknown => return None,
                };
                let span = if closing {
                    el.close.as_ref().map(|c| c.name).unwrap_or(el.name)
                } else {
                    el.name
                };
                let (start, end) = doc.utf16_span(span);
                Some(QuickInfo { contents, start, end })
            }
            Position::AttrName { el, attr } => {
                let tag = el.name.text(text);
                let name = attr.name.text(text);
                let knowledge = self.registry.lookup(tag, el.kind, &self.config);
                let contents = self.attribute_hover(tag, name, attr, &knowledge)?;
                let (start, end) = doc.utf16_span(attr.name);
                Some(QuickInfo { contents, start, end })
            }
            _ => None,
        }
    }

    fn directive_arg_hover(&self, doc: &DocumentState, offset: u32) -> Option<QuickInfo> {
        let ph = doc.fact.placeholders.iter().find(|p| {
            p.expr
                .as_ref()
                .and_then(|e| e.directive.as_ref())
                .and_then(|d| d.arg_start.zip(d.arg_end))
                .map(|(s, e)| s <= offset && offset <= e)
                .unwrap_or(false)
        })?;
        let directive = ph.expr.as_ref()?.directive.as_ref()?;
        let arg = directive.arg_string.as_deref()?;
        let member = doc
            .fact
            .source_members
            .as_ref()?
            .iter()
            .find(|m| m.name == arg)?;
        let type_text = member.type_text.as_deref().unwrap_or("unknown");
        let source = doc.fact.source_type_name.as_deref().unwrap_or("TSource");
        let mut contents = format!("```ts\n{source}.{arg}: {type_text}\n```");
        if let Some(docs) = &member.documentation {
            if !docs.is_empty() {
                contents.push_str("\n\n");
                contents.push_str(docs);
            }
        }
        Some(QuickInfo {
            contents,
            start: directive.arg_start?,
            end: directive.arg_end?,
        })
    }

    fn attribute_hover(
        &self,
        tag: &str,
        name: &str,
        attr: &Attribute,
        knowledge: &TagKnowledge<'_>,
    ) -> Option<String> {
        match attr.modifier.map(|(m, _)| m) {
            None | Some(Modifier::Boolean) => match knowledge {
                TagKnowledge::Components(comps) => {
                    let member = self.registry.find_attribute(comps, name)?;
                    let type_text = member.type_text.as_deref().unwrap_or("string");
                    let mode = member.mode.as_deref().unwrap_or("reflect");
                    let mut out = format!(
                        "```ts\n<{tag} {name}>: {type_text}\n```\n\nmode: `{mode}`"
                    );
                    if let Some(docs) = &member.documentation {
                        if !docs.is_empty() {
                            out.push_str("\n\n");
                            out.push_str(docs);
                        }
                    }
                    Some(out)
                }
                TagKnowledge::Builtin(data) => data
                    .attributes
                    .iter()
                    .find(|a| a.name == name)
                    .map(|a| a.description.to_string())
                    .or_else(|| {
                        fast_html_data::global_attribute(name).map(|a| a.description.to_string())
                    })
                    .or_else(|| {
                        fast_html_data::svg_presentation_attribute(name)
                            .map(|a| a.description.to_string())
                    }),
                TagKnowledge::Custom(custom) => custom
                    .attributes
                    .iter()
                    .find(|a| a.name == name)
                    .map(|a| a.description.clone()),
                _ => None,
            },
            Some(Modifier::Property) => match knowledge {
                TagKnowledge::Components(comps) => {
                    let member = self.registry.find_property(comps, name)?;
                    let type_text = member.type_text.as_deref().unwrap_or("unknown");
                    let mut out = format!("```ts\n{name}: {type_text}\n```");
                    if let Some(docs) = &member.documentation {
                        if !docs.is_empty() {
                            out.push_str("\n\n");
                            out.push_str(docs);
                        }
                    }
                    Some(out)
                }
                _ => Some(format!("Property binding: `{tag}.{name} = value`")),
            },
            Some(Modifier::Event) => {
                let declared = match knowledge {
                    TagKnowledge::Components(comps) => self.registry.find_event(comps, name),
                    _ => None,
                }
                .or_else(|| self.registry.global_event(name));
                match declared {
                    Some(event) => {
                        let detail = event.type_text.as_deref().unwrap_or("any");
                        let mut out = format!("```ts\n@{name} — CustomEvent<{detail}>\n```");
                        if let Some(docs) = &event.documentation {
                            if !docs.is_empty() {
                                out.push_str("\n\n");
                                out.push_str(docs);
                            }
                        }
                        Some(out)
                    }
                    None => fast_html_data::event(name).map(str::to_string),
                }
            }
        }
    }

    // ------------------------------------------------------------ definition

    fn definition(&self, document_id: &str, offset: u32) -> Option<DefinitionResult> {
        let doc = self.document(document_id)?;
        let tree = doc.tree.as_ref()?;
        let byte = doc.byte_of_utf16(offset);
        let text = doc.text();

        if let Some((directive, member)) = self.directive_arg_member(doc, offset) {
            let target = member.decl_span.clone()?;
            return Some(DefinitionResult {
                targets: vec![target],
                origin_start: directive.arg_start?,
                origin_end: directive.arg_end?,
                name: member.name.clone(),
            });
        }

        match resolve_position(&tree.children, byte, None) {
            Position::TagName { el, closing } => {
                let tag = el.name.text(text);
                let comps = self.registry.components_for_tag(tag);
                let targets: Vec<FileSpan> = comps
                    .iter()
                    .filter_map(|c| c.decl_span.clone())
                    .collect();
                if targets.is_empty() {
                    return None;
                }
                let span = if closing {
                    el.close.as_ref().map(|c| c.name).unwrap_or(el.name)
                } else {
                    el.name
                };
                let (start, end) = doc.utf16_span(span);
                Some(DefinitionResult {
                    targets,
                    origin_start: start,
                    origin_end: end,
                    name: tag.to_string(),
                })
            }
            Position::AttrName { el, attr } => {
                let tag = el.name.text(text);
                let name = attr.name.text(text);
                let comps = self.registry.components_for_tag(tag);
                let modifier = attr.modifier.map(|(m, _)| m);
                // An event may be declared globally, so it resolves on a
                // built-in tag too; everything else needs a component.
                if comps.is_empty() && !matches!(modifier, Some(Modifier::Event)) {
                    return None;
                }
                let decl = match modifier {
                    None | Some(Modifier::Boolean) => self
                        .registry
                        .find_attribute(&comps, name)
                        .and_then(|m| m.decl_span.clone()),
                    Some(Modifier::Property) => self
                        .registry
                        .find_property(&comps, name)
                        .and_then(|m| m.decl_span.clone()),
                    Some(Modifier::Event) => self
                        .registry
                        .find_event(&comps, name)
                        .or_else(|| self.registry.global_event(name))
                        .and_then(|e| e.decl_span.clone()),
                }?;
                let (start, end) = doc.utf16_span(attr.name);
                Some(DefinitionResult {
                    targets: vec![decl],
                    origin_start: start,
                    origin_end: end,
                    name: name.to_string(),
                })
            }
            Position::AttrValue { el, attr, parent } => {
                let name = attr.name.text(text);
                if attr.modifier.is_none() && name == "slot" {
                    let parent = parent?;
                    let parent_tag = parent.name.text(text);
                    let comps = self.registry.components_for_tag(parent_tag);
                    let value = attr.value.as_ref()?;
                    let slot_name = value.span.text(text);
                    let slot = comps
                        .iter()
                        .flat_map(|c| c.slots.iter())
                        .find(|s| s.name == slot_name)?;
                    let target = slot.decl_span.clone()?;
                    let (start, end) = doc.utf16_span(value.span);
                    return Some(DefinitionResult {
                        targets: vec![target],
                        origin_start: start,
                        origin_end: end,
                        name: slot_name.to_string(),
                    });
                }
                let _ = el;
                None
            }
            _ => None,
        }
    }

    fn directive_arg_member<'d>(
        &self,
        doc: &'d DocumentState,
        offset: u32,
    ) -> Option<(&'d crate::protocol::DirectiveInfo, &'d crate::protocol::SourceMember)> {
        let ph = doc.fact.placeholders.iter().find(|p| {
            p.expr
                .as_ref()
                .and_then(|e| e.directive.as_ref())
                .and_then(|d| d.arg_start.zip(d.arg_end))
                .map(|(s, e)| s <= offset && offset <= e)
                .unwrap_or(false)
        })?;
        let directive = ph.expr.as_ref()?.directive.as_ref()?;
        let arg = directive.arg_string.as_deref()?;
        let member = doc
            .fact
            .source_members
            .as_ref()?
            .iter()
            .find(|m| m.name == arg)?;
        Some((directive, member))
    }

    // ------------------------------------------------------------ references

    fn references_at(&self, document_id: &str, offset: u32) -> Option<Vec<FileSpan>> {
        let doc = self.document(document_id)?;
        let tree = doc.tree.as_ref()?;
        let byte = doc.byte_of_utf16(offset);
        let text = doc.text();

        if let Some((_, member)) = self.directive_arg_member(doc, offset) {
            let comp = doc
                .fact
                .source_type_id
                .and_then(|id| self.registry.component_by_source_type(id));
            return Some(self.member_locations(
                comp.and_then(|c| c.tag_name.as_deref()),
                doc.fact.source_type_id,
                &member.name.clone(),
            ));
        }

        match resolve_position(&tree.children, byte, None) {
            Position::TagName { el, .. } => {
                let tag = el.name.text(text);
                Some(self.tag_locations(tag, false))
            }
            Position::AttrName { el, attr } => {
                let tag = el.name.text(text);
                let name = attr.name.text(text);
                let comps = self.registry.components_for_tag(tag);
                if comps.is_empty() {
                    return None;
                }
                let source_type_id = comps.first().and_then(|c| c.source_type_id);
                Some(self.member_locations(Some(tag), source_type_id, name))
            }
            _ => None,
        }
    }

    /// Every `<tag>` and `</tag>` in every document; with `include_decl`, the
    /// registration's tag-name string too (for rename).
    fn tag_locations(&self, tag: &str, include_decl: bool) -> Vec<FileSpan> {
        let mut out = Vec::new();
        for doc in self.documents.values() {
            let Some(tree) = &doc.tree else { continue };
            tree.visit_elements(&mut |el, _| {
                if el.name.text(doc.text()) != tag {
                    return;
                }
                let (s, e) = doc.utf16_span(el.name);
                out.push(FileSpan {
                    file_name: doc.fact.file_name.clone(),
                    start: doc.absolute(s),
                    end: doc.absolute(e),
                });
                if let Some(close) = &el.close {
                    let (s, e) = doc.utf16_span(close.name);
                    out.push(FileSpan {
                        file_name: doc.fact.file_name.clone(),
                        start: doc.absolute(s),
                        end: doc.absolute(e),
                    });
                }
            });
        }
        if include_decl {
            for comp in self.registry.components_for_tag(tag) {
                if let Some(span) = &comp.tag_name_span {
                    out.push(span.clone());
                }
            }
        }
        out
    }

    /// Template occurrences of a member: `name=`/`?name=` attribute bindings
    /// and `:name` property bindings on the tag, `@name` event bindings, and
    /// `ref('name')`-family strings in documents whose source type matches.
    fn member_locations(
        &self,
        tag: Option<&str>,
        source_type_id: Option<u32>,
        name: &str,
    ) -> Vec<FileSpan> {
        let mut out = Vec::new();
        for doc in self.documents.values() {
            let text = doc.text();
            if let Some(tree) = &doc.tree {
                if let Some(tag) = tag {
                    tree.visit_elements(&mut |el, _| {
                        if el.name.text(text) != tag {
                            return;
                        }
                        for attr in &el.attributes {
                            if attr.name.text(text) != name {
                                continue;
                            }
                            let (s, e) = doc.utf16_span(attr.name);
                            out.push(FileSpan {
                                file_name: doc.fact.file_name.clone(),
                                start: doc.absolute(s),
                                end: doc.absolute(e),
                            });
                        }
                    });
                }
            }
            if source_type_id.is_some() && doc.fact.source_type_id == source_type_id {
                for ph in &doc.fact.placeholders {
                    let Some(directive) = ph.expr.as_ref().and_then(|e| e.directive.as_ref())
                    else {
                        continue;
                    };
                    if directive.arg_string.as_deref() == Some(name) {
                        if let (Some(s), Some(e)) = (directive.arg_start, directive.arg_end) {
                            out.push(FileSpan {
                                file_name: doc.fact.file_name.clone(),
                                start: doc.absolute(s),
                                end: doc.absolute(e),
                            });
                        }
                    }
                }
            }
        }
        out
    }

    // ---------------------------------------------------------------- rename

    fn rename_info(&self, document_id: &str, offset: u32) -> Option<RenameInfo> {
        let doc = self.document(document_id)?;
        let tree = doc.tree.as_ref()?;
        let byte = doc.byte_of_utf16(offset);
        let text = doc.text();

        if let Some((directive, member)) = self.directive_arg_member(doc, offset) {
            return Some(RenameInfo {
                can_rename: true,
                display_name: member.name.clone(),
                trigger_start: directive.arg_start?,
                trigger_end: directive.arg_end?,
                kind: "member".into(),
                tag: doc
                    .fact
                    .source_type_id
                    .and_then(|id| self.registry.component_by_source_type(id))
                    .and_then(|c| c.tag_name.clone()),
                member: Some(member.name.clone()),
                source_type_id: doc.fact.source_type_id,
            });
        }

        match resolve_position(&tree.children, byte, None) {
            Position::TagName { el, closing } => {
                let tag = el.name.text(text);
                if self.registry.components_for_tag(tag).is_empty() {
                    return None;
                }
                let span = if closing {
                    el.close.as_ref().map(|c| c.name).unwrap_or(el.name)
                } else {
                    el.name
                };
                let (start, end) = doc.utf16_span(span);
                Some(RenameInfo {
                    can_rename: true,
                    display_name: tag.to_string(),
                    trigger_start: start,
                    trigger_end: end,
                    kind: "tag".into(),
                    tag: Some(tag.to_string()),
                    member: None,
                    source_type_id: None,
                })
            }
            Position::AttrName { el, attr } => {
                let tag = el.name.text(text);
                let name = attr.name.text(text);
                let comps = self.registry.components_for_tag(tag);
                if comps.is_empty() {
                    return None;
                }
                let known = match attr.modifier.map(|(m, _)| m) {
                    None | Some(Modifier::Boolean) => {
                        self.registry.find_attribute(&comps, name).is_some()
                    }
                    Some(Modifier::Property) => {
                        self.registry.find_property(&comps, name).is_some()
                    }
                    Some(Modifier::Event) => self.registry.find_event(&comps, name).is_some(),
                };
                if !known {
                    return None;
                }
                let (start, end) = doc.utf16_span(attr.name);
                Some(RenameInfo {
                    can_rename: true,
                    display_name: name.to_string(),
                    trigger_start: start,
                    trigger_end: end,
                    kind: "member".into(),
                    tag: Some(tag.to_string()),
                    member: Some(name.to_string()),
                    source_type_id: comps.first().and_then(|c| c.source_type_id),
                })
            }
            _ => None,
        }
    }

    fn rename_locations_at(&self, document_id: &str, offset: u32) -> Option<Vec<FileSpan>> {
        let info = self.rename_info(document_id, offset)?;
        match info.kind.as_str() {
            "tag" => Some(self.tag_locations(info.tag.as_deref()?, true)),
            "member" => Some(self.member_locations(
                info.tag.as_deref(),
                info.source_type_id,
                info.member.as_deref()?,
            )),
            _ => None,
        }
    }

    // ------------------------------------------------- closing tag, folding

    fn closing_tag(&self, document_id: &str, offset: u32) -> Option<ClosingTagResult> {
        let doc = self.document(document_id)?;
        let tree = doc.tree.as_ref()?;
        let byte = doc.byte_of_utf16(offset);
        let mut found: Option<String> = None;
        tree.visit_elements(&mut |el, _| {
            if found.is_some() {
                return;
            }
            if el.open.end == byte
                && !el.is_void
                && !el.self_closing
                && el.close.is_none()
                && !el.closed_implicitly
            {
                found = Some(el.name.text(doc.text()).to_string());
            }
        });
        found.map(|name| ClosingTagResult {
            new_text: format!("</{name}>"),
        })
    }

    fn folding(&self, document_id: &str) -> Option<Vec<FoldingRange>> {
        let doc = self.document(document_id)?;
        let tree = doc.tree.as_ref()?;
        let text = doc.text();
        let mut out = Vec::new();
        tree.visit_elements(&mut |el, _| {
            let Some(close) = &el.close else { return };
            let inner = &text[el.open.end..close.span.start];
            if inner.contains('\n') {
                let (start, end) = doc.utf16_span(el.span());
                out.push(FoldingRange { start, end });
            }
        });
        Some(out)
    }

    // ------------------------------------------------------------ code fixes

    fn code_fixes(&self, document_id: &str, start: u32, end: u32) -> Option<Vec<crate::protocol::Fix>> {
        let result = self.analyze(document_id)?;
        let fixes = result
            .diagnostics
            .into_iter()
            .filter(|d| d.start <= end && start <= d.end)
            .flat_map(|d| d.fixes)
            .collect();
        Some(fixes)
    }

    // ---------------------------------------------------------- info + files

    fn document_info_at(&self, document_id: &str, offset: u32) -> Option<DocumentInfoAt> {
        let doc = self.document(document_id)?;
        let mut info = DocumentInfoAt {
            in_template: true,
            ..DocumentInfoAt::default()
        };
        for ph in &doc.fact.placeholders {
            if ph.start <= offset && offset <= ph.end {
                info.in_placeholder = Some(ph.index);
                if let Some(directive) = ph.expr.as_ref().and_then(|e| e.directive.as_ref()) {
                    if let (Some(s), Some(e)) = (directive.arg_start, directive.arg_end) {
                        if s <= offset && offset <= e {
                            info.directive_name = Some(directive.name.clone());
                            info.directive_arg_start = Some(s);
                            info.directive_arg_end = Some(e);
                        }
                    }
                }
            }
        }
        Some(info)
    }

    pub fn file_diagnostics(&self, file_name: &str) -> Vec<FileDiagnostic> {
        let mut out = Vec::new();
        let components = self.registry.components_in_file(file_name);
        for comp in components {
            let Some(tag) = comp.tag_name.as_deref() else {
                continue;
            };
            if let Some(span) = &comp.tag_name_span {
                if let Some(reason) = invalid_tag_name_reason(tag) {
                    if let Some(severity) =
                        crate::config::resolve_severity(&self.config, "no-invalid-tag-name")
                    {
                        out.push(FileDiagnostic {
                            rule_id: "no-invalid-tag-name".into(),
                            severity,
                            message: format!("'{tag}' is not a valid custom-element name: {reason}"),
                            file_name: span.file_name.clone(),
                            start: span.start,
                            end: span.end,
                        });
                    }
                }
                // Duplicate registration: FAST throws at define time.
                let all = self.registry.components_for_tag(tag);
                if all.len() > 1 {
                    if let Some(severity) =
                        crate::config::resolve_severity(&self.config, "no-duplicate-tag-name")
                    {
                        let others: Vec<String> = all
                            .iter()
                            .filter(|c| !std::ptr::eq(**c, comp))
                            .filter_map(|c| {
                                c.decl_span.as_ref().map(|s| {
                                    format!("{} ({})", c.class_name, s.file_name)
                                })
                            })
                            .collect();
                        out.push(FileDiagnostic {
                            rule_id: "no-duplicate-tag-name".into(),
                            severity,
                            message: format!(
                                "'{tag}' is registered more than once — FAST throws at registration time. Also registered by {}.",
                                others.join(", ")
                            ),
                            file_name: span.file_name.clone(),
                            start: span.start,
                            end: span.end,
                        });
                    }
                }
            }
        }
        out
    }
}

fn json<T: serde::Serialize>(value: Option<T>) -> serde_json::Value {
    match value {
        Some(v) => serde_json::to_value(v).unwrap_or(serde_json::Value::Null),
        None => serde_json::Value::Null,
    }
}

fn component_hover(tag: &str, comp: &ComponentFact) -> String {
    let mut out = format!("```ts\n<{tag}> — class {}\n```", comp.class_name);
    if let Some(docs) = &comp.documentation {
        if !docs.is_empty() {
            out.push_str("\n\n");
            out.push_str(docs);
        }
    }
    if !comp.slots.is_empty() {
        let names: Vec<String> = comp
            .slots
            .iter()
            .map(|s| {
                if s.name.is_empty() {
                    "(default)".into()
                } else {
                    format!("`{}`", s.name)
                }
            })
            .collect();
        out.push_str(&format!("\n\nSlots: {}", names.join(", ")));
    }
    if !comp.css_parts.is_empty() {
        let names: Vec<String> = comp.css_parts.iter().map(|p| format!("`{}`", p.name)).collect();
        out.push_str(&format!("\n\nCSS parts: {}", names.join(", ")));
    }
    if !comp.css_properties.is_empty() {
        let names: Vec<String> = comp
            .css_properties
            .iter()
            .map(|p| format!("`{}`", p.name))
            .collect();
        out.push_str(&format!("\n\nCSS custom properties: {}", names.join(", ")));
    }
    if !comp.has_shadow_root {
        out.push_str("\n\nRenders into the light DOM (`shadowOptions: null`).");
    }
    out
}

/// <https://html.spec.whatwg.org/multipage/custom-elements.html#valid-custom-element-name>,
/// the practically-checkable part.
fn invalid_tag_name_reason(tag: &str) -> Option<&'static str> {
    const RESERVED: &[&str] = &[
        "annotation-xml", "color-profile", "font-face", "font-face-src",
        "font-face-uri", "font-face-format", "font-face-name", "missing-glyph",
    ];
    if RESERVED.contains(&tag) {
        return Some("the name is reserved by the HTML specification");
    }
    if !tag.contains('-') {
        return Some("a custom-element name must contain a hyphen");
    }
    if tag.chars().any(|c| c.is_ascii_uppercase()) {
        return Some("a custom-element name must be lowercase");
    }
    let mut chars = tag.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return Some("the name must start with a lowercase ASCII letter"),
    }
    if tag
        .chars()
        .any(|c| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.' || c == '_'))
    {
        return Some("the name contains characters not allowed in a custom-element name");
    }
    None
}

fn enclosing_kind(nodes: &[Node], offset: usize) -> ElementKind {
    for node in nodes {
        if let Node::Element(el) = node {
            if el.span().contains(offset) {
                let inner = enclosing_kind(&el.children, offset);
                if matches!(inner, ElementKind::Html) {
                    return match el.kind {
                        ElementKind::Svg => ElementKind::Svg,
                        ElementKind::MathMl => ElementKind::MathMl,
                        _ => ElementKind::Html,
                    };
                }
                return inner;
            }
        }
    }
    ElementKind::Html
}

fn collect_unclosed(nodes: &[Node], offset: usize, text: &str, out: &mut Vec<String>) {
    for node in nodes {
        if let Node::Element(el) = node {
            let span = el.span();
            if span.contains(offset) || span.end <= offset && el.close.is_none() {
                if el.close.is_none() && !el.is_void && !el.self_closing {
                    out.push(el.name.text(text).to_string());
                }
                collect_unclosed(&el.children, offset, text, out);
            }
        }
    }
}
