//! The rule pass: one walk over a parsed virtual document, every structural
//! rule decided here, and a binding fact emitted for every question that
//! belongs to the type checker (design/rules.md, architecture.md §4.3).

use fast_template_syntax::{
    AttrPart, Attribute, Element, ElementKind, Modifier, Node, ParseErrorKind, Span,
};

use crate::config::resolve_severity;
use crate::documents::DocumentState;
use crate::protocol::{
    AnalyzeResult, BindingFact, Config, Diagnostic, Edit, ExprInfo, Fix, FixCommand, Severity,
};
use crate::registry::{Registry, TagKnowledge, TagOrigin};
use crate::suggest;

/// DOM properties commonly bound with `:` on built-in elements that have no
/// attribute of the same name. Attributes cover the rest: an attribute named
/// `value` is evidence enough for a property named `value`.
pub(crate) const DOM_EXTRA_PROPERTIES: &[&str] = &[
    "value", "checked", "disabled", "readOnly", "innerHTML", "innerText",
    "textContent", "className", "classList", "id", "title", "hidden",
    "selected", "currentTime", "volume", "muted", "open", "scrollTop",
    "scrollLeft", "indeterminate", "srcObject", "valueAsNumber",
    "valueAsDate", "files", "htmlFor",
];

/// Attributes legal on any element that the generated global table does not
/// carry (`part`/`exportparts` are shadow-DOM plumbing VS Code's data lacks).
const ALWAYS_GLOBAL_ATTRIBUTES: &[&str] = &["part", "exportparts", "is", "key"];

const CONTENT_DIRECTIVES: &[&str] = &["when", "repeat", "render"];
const ELEMENT_DIRECTIVES: &[&str] = &["ref", "slotted", "children"];

pub fn analyze_document(
    doc: &DocumentState,
    registry: &Registry,
    config: &Config,
) -> AnalyzeResult {
    let mut pass = Pass {
        doc,
        registry,
        config,
        diagnostics: Vec::new(),
        facts: Vec::new(),
    };
    pass.run();
    AnalyzeResult {
        diagnostics: pass.diagnostics,
        facts: pass.facts,
    }
}

struct Pass<'a> {
    doc: &'a DocumentState,
    registry: &'a Registry,
    config: &'a Config,
    diagnostics: Vec<Diagnostic>,
    facts: Vec<BindingFact>,
}

impl<'a> Pass<'a> {
    fn run(&mut self) {
        if self.doc.fact.kind != "html" {
            return;
        }

        if self.doc.uses_partial {
            for ph in &self.doc.fact.placeholders {
                if ph.expr.as_ref().map(|e| e.is_partial).unwrap_or(false) {
                    self.diagnostics.push(Diagnostic {
                        rule_id: "template-not-analyzed".into(),
                        severity: Severity::Suggestion,
                        message: "html.partial(…) interpolates raw HTML, so this template is not analyzed.".into(),
                        start: ph.start,
                        end: ph.end,
                        origin: None,
                        fixes: Vec::new(),
                    });
                }
            }
            return;
        }

        self.check_untyped_template();

        let Some(tree) = &self.doc.tree else { return };

        for error in &tree.errors {
            match error.kind {
                ParseErrorKind::StrayCloseTag => {
                    self.report(
                        "no-unclosed-tag",
                        error.span,
                        format!("Close tag </{}> has no matching open tag.", error.tag),
                        Vec::new(),
                    );
                }
                // Unclosed and self-closed elements are reported from the
                // element visit, where the fix has the tree to work with.
                ParseErrorKind::UnclosedTag | ParseErrorKind::SelfClosedNonVoid => {}
            }
        }

        let roots = &tree.children;
        self.visit_nodes(roots, None);
    }

    // ------------------------------------------------------------- plumbing

    fn severity(&self, rule_id: &str) -> Option<Severity> {
        resolve_severity(self.config, rule_id)
    }

    fn enabled(&self, rule_id: &str) -> bool {
        self.severity(rule_id).is_some()
    }

    /// Report at a byte span.
    fn report(&mut self, rule_id: &str, span: Span, message: String, fixes: Vec<Fix>) {
        self.report_with_origin(rule_id, span, message, fixes, None);
    }

    fn report_with_origin(
        &mut self,
        rule_id: &str,
        span: Span,
        message: String,
        fixes: Vec<Fix>,
        origin: Option<TagOrigin>,
    ) {
        let Some(severity) = self.severity(rule_id) else {
            return;
        };
        let (start, end) = self.doc.utf16_span(span);
        self.diagnostics.push(Diagnostic {
            rule_id: rule_id.to_string(),
            severity,
            message,
            start,
            end,
            origin: origin.map(|o| o.as_str().to_string()),
            fixes,
        });
    }

    fn suggestion_tail(&self, suggestion: Option<&str>) -> String {
        suggest::did_you_mean(suggestion, self.config.dont_show_suggestions)
    }

    fn rename_fix(&self, span: Span, to: &str) -> Fix {
        let (start, end) = self.doc.utf16_span(span);
        Fix {
            label: format!("Rename to '{to}'"),
            edits: vec![Edit {
                file_name: None,
                start,
                end,
                new_text: to.to_string(),
            }],
            command: None,
        }
    }

    fn expr(&self, index: u32) -> Option<&'a ExprInfo> {
        self.doc.placeholder_fact(index)?.expr.as_ref()
    }

    /// The tag of the component this document belongs to: marked on the
    /// document when registration and template share a file, resolved through
    /// the registry when they do not.
    fn component_tag(&self) -> Option<String> {
        if let Some(tag) = self.doc.fact.component_tag.as_deref() {
            return Some(tag.to_string());
        }
        self.registry
            .component_for_document(&self.doc.fact.id)
            .and_then(|c| c.tag_name.clone())
    }

    fn is_directive_expr(&self, index: u32) -> bool {
        self.expr(index)
            .map(|e| e.directive.is_some() || e.is_directive_value)
            .unwrap_or(false)
    }

    // ------------------------------------------------------------- the walk

    fn visit_nodes(&mut self, nodes: &'a [Node], parent: Option<&'a Element>) {
        for node in nodes {
            match node {
                Node::Element(el) => {
                    self.visit_element(el, parent);
                    self.visit_nodes(&el.children, Some(el));
                }
                Node::Placeholder(ph) => self.check_content_placeholder(ph.index, ph.span),
                Node::Text(_) | Node::Comment(_) | Node::Doctype(_) => {}
            }
        }
    }

    fn visit_element(&mut self, el: &'a Element, parent: Option<&'a Element>) {
        let text = self.doc.text();
        let name = el.name.text(text);
        if name.is_empty() {
            return;
        }

        self.check_closedness(el, name);

        let knowledge = self.registry.lookup(name, el.kind, self.config);

        if matches!(knowledge, TagKnowledge::Unknown) {
            let suggestion = self.suggest_tag(name, el.kind);
            let fixes = suggestion
                .as_deref()
                .map(|s| {
                    let mut fix = self.rename_fix(el.name, s);
                    if let Some(close) = &el.close {
                        let (cs, ce) = self.doc.utf16_span(close.name);
                        fix.edits.push(Edit {
                            file_name: None,
                            start: cs,
                            end: ce,
                            new_text: s.to_string(),
                        });
                    }
                    fix
                })
                .into_iter()
                .collect();
            let tail = self.suggestion_tail(suggestion.as_deref());
            self.report(
                "no-unknown-tag-name",
                el.name,
                format!("Unknown tag <{name}>.{tail}"),
                fixes,
            );
        }

        if let TagKnowledge::Components(_) = &knowledge {
            self.check_missing_import(el, name);
        }

        if name.eq_ignore_ascii_case("slot") {
            self.check_light_dom_slot(el);
        }

        for attr in &el.attributes {
            self.visit_attribute(el, attr, name, &knowledge, parent);
        }

        for ph in &el.element_expressions {
            self.check_element_expression(el, ph.index, ph.span);
        }
    }

    fn check_closedness(&mut self, el: &Element, name: &str) {
        if el.is_void || el.closed_implicitly {
            return;
        }
        if el.self_closing {
            if !matches!(el.kind, ElementKind::Svg | ElementKind::MathMl) {
                let open_end = self.doc.utf16_of_byte(el.open.end);
                let fix = Fix {
                    label: format!("Close <{name}> with a real close tag"),
                    edits: vec![Edit {
                        file_name: None,
                        start: open_end.saturating_sub(2),
                        end: open_end,
                        new_text: format!("></{name}>"),
                    }],
                    command: None,
                };
                self.report(
                    "no-unclosed-tag",
                    el.name,
                    format!(
                        "<{name}> cannot be self-closed — only void elements and foreign content can. FAST parses this as an unclosed element."
                    ),
                    vec![fix],
                );
            }
            return;
        }
        if el.close.is_none() {
            let insert_at = self.doc.utf16_of_byte(el.inner_end());
            let fix = Fix {
                label: format!("Insert </{name}>"),
                edits: vec![Edit {
                    file_name: None,
                    start: insert_at,
                    end: insert_at,
                    new_text: format!("</{name}>"),
                }],
                command: None,
            };
            self.report(
                "no-unclosed-tag",
                el.name,
                format!("<{name}> was never closed."),
                vec![fix],
            );
        }
    }

    fn check_missing_import(&mut self, el: &Element, tag: &str) {
        if !self.enabled("no-missing-import") {
            return;
        }
        // A component known only through `HTMLElementTagNameMap` has no
        // module of ours to import: the augmentation is ambient and the
        // registration happens wherever the library was told to run it.
        let components = self.registry.components_for_tag(tag);
        if !components.is_empty() && components.iter().all(|c| c.origin == "tagNameMap") {
            return;
        }
        let reachable = self.registry.is_reachable(
            &self.doc.fact.file_name,
            tag,
            self.config.max_project_import_depth,
        );
        if reachable {
            return;
        }
        let declaring = self.registry.declaring_files(tag);
        let from = declaring.first().copied().unwrap_or("another module");
        let fix = Fix {
            label: format!("Import the module that declares <{tag}>"),
            edits: Vec::new(),
            command: Some(FixCommand {
                kind: "addImport".into(),
                target_file: declaring.first().map(|f| f.to_string()),
            }),
        };
        self.report(
            "no-missing-import",
            el.name,
            format!(
                "<{tag}> is declared in {from}, which is not reachable from this module's imports — the element will not be defined when this template renders alone."
            ),
            vec![fix],
        );
    }

    fn check_light_dom_slot(&mut self, el: &Element) {
        let Some(component_tag) = self.component_tag() else {
            return;
        };
        let comps = self.registry.components_for_tag(&component_tag);
        if !comps.is_empty() && comps.iter().all(|c| !c.has_shadow_root) {
            self.report(
                "no-slot-without-shadow-root",
                el.name,
                format!(
                    "<slot> does nothing here: <{component_tag}> renders into the light DOM (shadowOptions: null), so nothing is ever projected."
                ),
                Vec::new(),
            );
        }
    }

    // ---------------------------------------------------------- attributes

    fn visit_attribute(
        &mut self,
        el: &'a Element,
        attr: &'a Attribute,
        tag: &str,
        knowledge: &TagKnowledge<'a>,
        parent: Option<&'a Element>,
    ) {
        let text = self.doc.text();
        let name = attr.name.text(text);
        if name.is_empty() {
            return;
        }

        match attr.modifier {
            None => self.plain_attribute(el, attr, tag, name, knowledge, parent),
            Some((Modifier::Property, _)) => {
                self.property_binding(el, attr, tag, name, knowledge)
            }
            Some((Modifier::Boolean, _)) => {
                self.boolean_binding(el, attr, tag, name, knowledge)
            }
            Some((Modifier::Event, _)) => self.event_binding(attr, tag, name, knowledge),
        }

        // Rules that apply to every binding form.
        if let Some(value) = &attr.value {
            self.check_mixed_binding(value);
            for part in &value.parts {
                if let AttrPart::Placeholder(ph) = part {
                    self.check_value_placeholder(ph.index, ph.span);
                }
            }
        }
    }

    fn plain_attribute(
        &mut self,
        el: &'a Element,
        attr: &'a Attribute,
        tag: &str,
        name: &str,
        knowledge: &TagKnowledge<'a>,
        parent: Option<&'a Element>,
    ) {
        let origin = knowledge.origin();
        if name == "slot" {
            self.check_slot_attribute(attr, parent);
        }

        if !self.attribute_is_known(el, name, knowledge) {
            let suggestion = self.suggest_attribute(el, name, knowledge);
            let fixes = suggestion
                .as_deref()
                .map(|s| self.rename_fix(attr.name, s))
                .into_iter()
                .collect();
            let tail = self.suggestion_tail(suggestion.as_deref());
            self.report_with_origin(
                "no-unknown-attribute",
                attr.name,
                format!("Unknown attribute '{name}' on <{tag}>.{tail}"),
                fixes,
                origin,
            );
        }

        // Type facts, when the binding carries an expression or targets a
        // typed member with a literal.
        let wants_facts = self.enabled("no-boolean-in-attribute-binding")
            || self.enabled("no-complex-attribute-binding")
            || self.enabled("no-incompatible-type-binding");
        if !wants_facts {
            return;
        }
        let Some(value) = &attr.value else { return };
        let (target, mode, builtin) = self.attribute_target(name, knowledge);
        if let Some(ph) = value.single_placeholder() {
            // A directive in value position is already R18's problem; asking
            // the oracle about its type would double-report it.
            if self.is_directive_expr(ph.index) {
                return;
            }
            let (start, end) = self.doc.utf16_span(attr.full);
            self.facts.push(BindingFact {
                kind: "attribute".into(),
                start,
                end,
                tag_name: tag.to_string(),
                member_name: Some(name.to_string()),
                target_declaration_id: target,
                target_kind: Some("attribute".into()),
                target_builtin: builtin,
                expression_index: Some(ph.index),
                literal: None,
                mode,
            });
        } else if value.is_literal_only() && target.is_some() {
            let (start, end) = self.doc.utf16_span(attr.full);
            self.facts.push(BindingFact {
                kind: "attribute".into(),
                start,
                end,
                tag_name: tag.to_string(),
                member_name: Some(name.to_string()),
                target_declaration_id: target,
                target_kind: Some("attribute".into()),
                target_builtin: builtin,
                expression_index: None,
                literal: Some(value.span.text(self.doc.text()).to_string()),
                mode,
            });
        }
    }

    fn property_binding(
        &mut self,
        _el: &'a Element,
        attr: &'a Attribute,
        tag: &str,
        name: &str,
        knowledge: &TagKnowledge<'a>,
    ) {
        let origin = knowledge.origin();
        // `:classList` is the tokenList aspect — the one attribute name whose
        // value changes the aspect. Always legal.
        let is_class_list = name == "classList";

        // R9: a property binding needs an expression.
        let has_expression = attr
            .value
            .as_ref()
            .map(|v| v.placeholders().next().is_some())
            .unwrap_or(false);
        if !has_expression {
            let (mod_start, mod_end) = self
                .doc
                .utf16_span(attr.modifier.map(|(_, s)| s).unwrap_or(attr.name));
            let fix = Fix {
                label: "Drop the ':' — set the attribute instead".into(),
                edits: vec![Edit {
                    file_name: None,
                    start: mod_start,
                    end: mod_end,
                    new_text: String::new(),
                }],
                command: None,
            };
            self.report(
                "no-expressionless-property-binding",
                attr.full,
                format!(
                    "':{name}' is a property binding, which needs an expression. A literal here assigns the literal string — drop the ':' to set the attribute instead."
                ),
                vec![fix],
            );
            return;
        }

        if !is_class_list && !self.property_is_known(name, knowledge) {
            let suggestion = self.suggest_property(name, knowledge);
            let fixes = suggestion
                .as_deref()
                .map(|s| self.rename_fix(attr.name, s))
                .into_iter()
                .collect();
            let tail = self.suggestion_tail(suggestion.as_deref());
            self.report_with_origin(
                "no-unknown-property",
                attr.name,
                format!("Unknown property ':{name}' on <{tag}>.{tail}"),
                fixes,
                origin,
            );
        }

        if !self.enabled("no-incompatible-type-binding") || is_class_list {
            return;
        }
        let Some(value) = &attr.value else { return };
        let Some(ph) = value.single_placeholder() else {
            return;
        };
        if self.is_directive_expr(ph.index) {
            return;
        }
        let (target, builtin) = self.property_target(name, knowledge);
        if target.is_none() && !builtin {
            return;
        }
        let (start, end) = self.doc.utf16_span(attr.full);
        self.facts.push(BindingFact {
            kind: "property".into(),
            start,
            end,
            tag_name: tag.to_string(),
            member_name: Some(name.to_string()),
            target_declaration_id: target,
            target_kind: Some("property".into()),
            target_builtin: builtin,
            expression_index: Some(ph.index),
            literal: None,
            mode: None,
        });
    }

    fn boolean_binding(
        &mut self,
        el: &'a Element,
        attr: &'a Attribute,
        tag: &str,
        name: &str,
        knowledge: &TagKnowledge<'a>,
    ) {
        let origin = knowledge.origin();
        if !self.attribute_is_known(el, name, knowledge) {
            let suggestion = self.suggest_attribute(el, name, knowledge);
            let fixes = suggestion
                .as_deref()
                .map(|s| self.rename_fix(attr.name, s))
                .into_iter()
                .collect();
            let tail = self.suggestion_tail(suggestion.as_deref());
            self.report_with_origin(
                "no-unknown-attribute",
                attr.name,
                format!("Unknown attribute '?{name}' on <{tag}>.{tail}"),
                fixes,
                origin,
            );
        }
        if !self.enabled("no-incompatible-type-binding") {
            return;
        }
        let Some(ph) = attr
            .value
            .as_ref()
            .and_then(|v| v.single_placeholder())
        else {
            return;
        };
        if self.is_directive_expr(ph.index) {
            return;
        }
        let (target, mode, builtin) = self.attribute_target(name, knowledge);
        let (start, end) = self.doc.utf16_span(attr.full);
        self.facts.push(BindingFact {
            kind: "booleanAttribute".into(),
            start,
            end,
            tag_name: tag.to_string(),
            member_name: Some(name.to_string()),
            target_declaration_id: target,
            target_kind: Some("attribute".into()),
            target_builtin: builtin,
            expression_index: Some(ph.index),
            literal: None,
            mode,
        });
    }

    fn event_binding(
        &mut self,
        attr: &'a Attribute,
        tag: &str,
        name: &str,
        knowledge: &TagKnowledge<'a>,
    ) {
        let origin = knowledge.origin();
        if !self.event_is_known(name, knowledge) {
            let suggestion = self.suggest_event(name, knowledge);
            let fixes = suggestion
                .as_deref()
                .map(|s| self.rename_fix(attr.name, s))
                .into_iter()
                .collect();
            let tail = self.suggestion_tail(suggestion.as_deref());
            self.report_with_origin(
                "no-unknown-event",
                attr.name,
                format!("Unknown event '@{name}' on <{tag}>.{tail}"),
                fixes,
                origin,
            );
        }

        let Some(value) = &attr.value else { return };
        match value.single_placeholder() {
            Some(ph) => {
                if self.is_directive_expr(ph.index) {
                    return;
                }
                if self.enabled("no-noncallable-event-binding")
                    || self.enabled("no-implicit-prevent-default")
                {
                    let (start, end) = self.doc.utf16_span(attr.full);
                    self.facts.push(BindingFact {
                        kind: "event".into(),
                        start,
                        end,
                        tag_name: tag.to_string(),
                        member_name: Some(name.to_string()),
                        target_declaration_id: None,
                        target_kind: Some("event".into()),
                        target_builtin: !matches!(knowledge, TagKnowledge::Components(_)),
                        expression_index: Some(ph.index),
                        literal: None,
                        mode: None,
                    });
                }
            }
            None => {
                // `@click="foo"` — a string is never callable; no oracle needed.
                if value.is_literal_only() && !value.span.text(self.doc.text()).is_empty() {
                    self.report(
                        "no-noncallable-event-binding",
                        attr.full,
                        format!(
                            "'@{name}' binds a literal string, which is not callable — an event binding needs a function expression."
                        ),
                        Vec::new(),
                    );
                }
            }
        }
    }

    fn check_slot_attribute(&mut self, attr: &'a Attribute, parent: Option<&'a Element>) {
        let Some(parent) = parent else { return };
        if !matches!(parent.kind, ElementKind::Custom) {
            return;
        }
        let parent_tag = parent.name.text(self.doc.text());
        let comps = self.registry.components_for_tag(parent_tag);
        if comps.is_empty() {
            return;
        }
        let slots: Vec<&str> = comps
            .iter()
            .flat_map(|c| c.slots.iter())
            .map(|s| s.name.as_str())
            .collect();
        if slots.is_empty() {
            // No @slot documentation: nothing to check against.
            return;
        }
        let Some(value) = &attr.value else { return };
        if !value.is_literal_only() {
            return;
        }
        let slot_name = value.span.text(self.doc.text());
        if slots.contains(&slot_name) {
            return;
        }
        let named: Vec<String> = slots
            .iter()
            .map(|s| {
                if s.is_empty() {
                    "(default)".to_string()
                } else {
                    format!("'{s}'")
                }
            })
            .collect();
        let suggestion = suggest::nearest(slot_name, slots.iter().copied());
        let tail = self.suggestion_tail(suggestion);
        self.report(
            "no-unknown-slot",
            value.span,
            format!(
                "<{parent_tag}> declares no slot named '{slot_name}'. Declared slots: {}.{tail}",
                named.join(", ")
            ),
            suggestion
                .map(|s| self.rename_fix(value.span, s))
                .into_iter()
                .collect(),
        );
    }

    fn check_mixed_binding(&mut self, value: &fast_template_syntax::AttributeValue) {
        if value.quote.is_some() {
            return;
        }
        let [AttrPart::Placeholder(_), AttrPart::Literal(lit)] = value.parts.as_slice() else {
            return;
        };
        let lit_text = lit.text(self.doc.text());
        if matches!(lit_text, "/" | "\"" | "'" | "}") {
            self.report(
                "no-unintended-mixed-binding",
                *lit,
                format!(
                    "The character '{lit_text}' was swept into this binding's value — it is almost certainly a typo next to the expression."
                ),
                Vec::new(),
            );
        }
    }

    // -------------------------------------------------------- placeholders

    /// Rules for a `${…}` in attribute-value position.
    fn check_value_placeholder(&mut self, index: u32, span: Span) {
        let Some(expr) = self.expr(index) else { return };
        if let Some(directive) = &expr.directive {
            if CONTENT_DIRECTIVES.contains(&directive.name.as_str()) {
                self.report(
                    "no-invalid-directive-binding",
                    span,
                    format!(
                        "{}(…) builds content — it can only be used in content position, not in an attribute value.",
                        directive.name
                    ),
                    Vec::new(),
                );
            } else if ELEMENT_DIRECTIVES.contains(&directive.name.as_str()) {
                self.report(
                    "no-invalid-directive-binding",
                    span,
                    format!(
                        "{}('…') is an element directive — place it between attributes (<div {}('…')>), not in an attribute value.",
                        directive.name, directive.name
                    ),
                    Vec::new(),
                );
            }
            return;
        }
        self.check_non_reactive(index, span);
    }

    /// Rules for a `${…}` in content position.
    fn check_content_placeholder(&mut self, index: u32, span: Span) {
        let Some(expr) = self.expr(index) else { return };
        if let Some(directive) = &expr.directive {
            if ELEMENT_DIRECTIVES.contains(&directive.name.as_str()) {
                self.report(
                    "no-invalid-directive-binding",
                    span,
                    format!(
                        "{}('…') attaches to an element — place it between attributes, not in content.",
                        directive.name
                    ),
                    Vec::new(),
                );
            }
            self.check_directive_target(expr, span);
            return;
        }
        self.check_non_reactive(index, span);
    }

    /// Rules for a `${…}` in attribute-name position — the element expression.
    fn check_element_expression(&mut self, el: &'a Element, index: u32, span: Span) {
        let Some(expr) = self.expr(index) else { return };
        let Some(directive) = &expr.directive else {
            return;
        };
        if CONTENT_DIRECTIVES.contains(&directive.name.as_str()) {
            self.report(
                "no-invalid-directive-binding",
                span,
                format!(
                    "{}(…) builds content — it can only be used in content position, not as an element expression.",
                    directive.name
                ),
                Vec::new(),
            );
            return;
        }
        if directive.name == "slotted" {
            let el_name = el.name.text(self.doc.text());
            if !el_name.eq_ignore_ascii_case("slot") {
                self.report(
                    "no-invalid-directive-binding",
                    span,
                    format!("slotted('…') observes a <slot>'s assigned nodes — it belongs on a <slot> element, not on <{el_name}>."),
                    Vec::new(),
                );
            }
            if let Some(component_tag) = self.component_tag() {
                let comps = self.registry.components_for_tag(&component_tag);
                if !comps.is_empty() && comps.iter().all(|c| !c.has_shadow_root) {
                    self.report(
                        "no-slot-without-shadow-root",
                        span,
                        format!(
                            "slotted('…') does nothing here: <{component_tag}> renders into the light DOM (shadowOptions: null)."
                        ),
                        Vec::new(),
                    );
                }
            }
        }
        self.check_directive_target(expr, span);
    }

    /// F2: `ref('…')` / `slotted('…')` / `children('…')` name a member of the
    /// template's source type.
    fn check_directive_target(&mut self, expr: &ExprInfo, _span: Span) {
        let Some(directive) = &expr.directive else {
            return;
        };
        if !ELEMENT_DIRECTIVES.contains(&directive.name.as_str()) {
            return;
        }
        let (Some(arg), Some(arg_start), Some(arg_end)) = (
            directive.arg_string.as_deref(),
            directive.arg_start,
            directive.arg_end,
        ) else {
            return;
        };
        let Some(members) = self.doc.fact.source_members.as_ref() else {
            return;
        };
        if members.iter().any(|m| m.name == arg) {
            return;
        }
        let Some(severity) = self.severity("no-invalid-directive-target") else {
            return;
        };
        let source = self
            .doc
            .fact
            .source_type_name
            .as_deref()
            .unwrap_or("the template's source type");
        let suggestion = suggest::nearest(arg, members.iter().map(|m| m.name.as_str()));
        let tail = self.suggestion_tail(suggestion);
        let fixes = suggestion
            .map(|s| Fix {
                label: format!("Rename to '{s}'"),
                edits: vec![Edit {
                    file_name: None,
                    start: arg_start,
                    end: arg_end,
                    new_text: s.to_string(),
                }],
                command: None,
            })
            .into_iter()
            .collect();
        self.diagnostics.push(Diagnostic {
            rule_id: "no-invalid-directive-target".into(),
            severity,
            message: format!(
                "'{arg}' is not a member of {source} — {}('{arg}') will silently never resolve.{tail}",
                directive.name
            ),
            start: arg_start,
            end: arg_end,
            origin: None,
            fixes,
        });
    }

    /// F1: a binding that is not a function, directive, template or constant
    /// is bound once, at view-creation time, and never updates.
    fn check_non_reactive(&mut self, index: u32, span: Span) {
        let Some(expr) = self.expr(index) else { return };
        if expr.is_directive_value || expr.directive.is_some() {
            return;
        }
        // The mistake's shape is reading a value — `${myEl.count}` — not a
        // computed call like `${shortcut('Ctrl+F', '⌘F')}`, which is a
        // deliberate one-time interpolation as often as not.
        if !matches!(expr.kind.as_str(), "identifier" | "propertyAccess") {
            return;
        }
        // Fire only when the plugin positively established both halves: the
        // type has no call signatures, and the symbol is not a constant. An
        // unknown (`None`) suppresses the rule rather than guessing.
        if expr.is_function_type != Some(false) || expr.is_constant != Some(false) {
            return;
        }
        let (start, end) = self.doc.utf16_span(span);
        let Some(severity) = self.severity("no-non-reactive-binding") else {
            return;
        };
        self.diagnostics.push(Diagnostic {
            rule_id: "no-non-reactive-binding".into(),
            severity,
            message: "This value is bound once, when the view is created — it will never update. Wrap it in an arrow (${x => …}) to make it reactive.".into(),
            start,
            end,
            origin: None,
            fixes: vec![Fix {
                label: "Wrap in an arrow function".into(),
                edits: vec![Edit {
                    file_name: None,
                    start: start + 2,
                    end: start + 2,
                    new_text: "() => ".into(),
                }],
                command: None,
            }],
        });
    }

    fn check_untyped_template(&mut self) {
        if self.doc.fact.source_type_id.is_some() || self.doc.fact.source_type_name.is_some() {
            return;
        }
        let Some(component_tag) = self.component_tag() else {
            return;
        };
        let Some(severity) = self.severity("no-untyped-template") else {
            return;
        };
        let class_name = self
            .registry
            .components_for_tag(&component_tag)
            .first()
            .map(|c| c.class_name.clone())
            .unwrap_or_else(|| "T".to_string());
        let fixes = self
            .doc
            .fact
            .type_arg_insert_offset
            .map(|offset| Fix {
                label: format!("Add the type argument html<{class_name}>"),
                edits: vec![Edit {
                    file_name: Some(self.doc.fact.file_name.clone()),
                    start: offset,
                    end: offset,
                    new_text: format!("<{class_name}>"),
                }],
                command: None,
            })
            .into_iter()
            .collect();
        self.diagnostics.push(Diagnostic {
            rule_id: "no-untyped-template".into(),
            severity,
            message: format!(
                "This is a component's template but it has no type argument, so nothing inside its bindings is checked — by TypeScript or by this analyzer. Write html<{class_name}>`…`."
            ),
            start: 0,
            end: 0,
            origin: None,
            fixes,
        });
    }

    // ------------------------------------------------------------- lookups

    fn attribute_is_known(
        &self,
        el: &Element,
        name: &str,
        knowledge: &TagKnowledge<'a>,
    ) -> bool {
        if !self.enabled("no-unknown-attribute") {
            return true;
        }
        if name.starts_with("data-") || name.contains("${") {
            return true;
        }
        if ALWAYS_GLOBAL_ATTRIBUTES.contains(&name)
            || self.config.global_attributes.iter().any(|a| a == name)
            || self.registry.custom_global_attribute(name)
        {
            return true;
        }
        let in_globals = fast_html_data::global_attribute(name).is_some();
        match knowledge {
            TagKnowledge::Components(comps) => {
                in_globals
                    || comps
                        .iter()
                        .flat_map(|c| c.attributes.iter())
                        .any(|a| a.name.eq_ignore_ascii_case(name))
            }
            TagKnowledge::Custom(tag) => {
                in_globals || tag.attributes.iter().any(|a| a.name == name)
            }
            TagKnowledge::GlobalTag | TagKnowledge::Unknown => true,
            TagKnowledge::Builtin(data) => {
                if data.attributes.iter().any(|a| a.name == name) {
                    return true;
                }
                match el.kind {
                    ElementKind::Svg | ElementKind::MathMl => {
                        fast_html_data::svg_presentation_attribute(name).is_some()
                            || in_globals
                            || name.starts_with("xlink:")
                            || name.starts_with("xml:")
                    }
                    _ => in_globals,
                }
            }
        }
    }

    fn property_is_known(&self, name: &str, knowledge: &TagKnowledge<'a>) -> bool {
        if !self.enabled("no-unknown-property") {
            return true;
        }
        match knowledge {
            TagKnowledge::Components(comps) => {
                self.registry.find_property(comps, name).is_some()
            }
            TagKnowledge::Custom(_) | TagKnowledge::GlobalTag | TagKnowledge::Unknown => true,
            TagKnowledge::Builtin(data) => {
                DOM_EXTRA_PROPERTIES.contains(&name)
                    || data
                        .attributes
                        .iter()
                        .any(|a| a.name.eq_ignore_ascii_case(name))
                    || fast_html_data::global_attribute(&name.to_ascii_lowercase()).is_some()
            }
        }
    }

    fn event_is_known(&self, name: &str, knowledge: &TagKnowledge<'a>) -> bool {
        if !self.enabled("no-unknown-event") {
            return true;
        }
        if fast_html_data::event(name).is_some()
            || self.config.global_events.iter().any(|e| e == name)
        {
            return true;
        }
        match knowledge {
            TagKnowledge::Components(comps) => self.registry.find_event(comps, name).is_some(),
            TagKnowledge::Custom(tag) => tag.events.iter().any(|e| e.name == name),
            TagKnowledge::GlobalTag | TagKnowledge::Unknown => true,
            TagKnowledge::Builtin(_) => false,
        }
    }

    fn attribute_target(
        &self,
        name: &str,
        knowledge: &TagKnowledge<'a>,
    ) -> (Option<u32>, Option<String>, bool) {
        match knowledge {
            TagKnowledge::Components(comps) => {
                match self.registry.find_attribute(comps, name) {
                    Some(member) => (
                        member.declaration_id,
                        member.mode.clone(),
                        false,
                    ),
                    None => (None, None, false),
                }
            }
            TagKnowledge::Builtin(_) => (None, None, true),
            _ => (None, None, false),
        }
    }

    fn property_target(&self, name: &str, knowledge: &TagKnowledge<'a>) -> (Option<u32>, bool) {
        match knowledge {
            TagKnowledge::Components(comps) => (
                self.registry
                    .find_property(comps, name)
                    .and_then(|m| m.declaration_id),
                false,
            ),
            TagKnowledge::Builtin(_) => (None, true),
            _ => (None, false),
        }
    }

    // ---------------------------------------------------------- suggesting

    fn suggest_tag(&self, name: &str, kind: ElementKind) -> Option<String> {
        let custom = self.registry.known_custom_tags(self.config);
        let namespace = match kind {
            ElementKind::Svg => fast_html_data::Namespace::Svg,
            ElementKind::MathMl => fast_html_data::Namespace::MathMl,
            _ => fast_html_data::Namespace::Html,
        };
        let builtins: Vec<&str> = fast_html_data::elements()
            .filter(|e| e.namespace == namespace)
            .map(|e| e.name)
            .collect();
        suggest::nearest(
            name,
            custom
                .iter()
                .map(String::as_str)
                .chain(builtins.iter().copied()),
        )
        .map(str::to_string)
    }

    fn suggest_attribute(
        &self,
        el: &Element,
        name: &str,
        knowledge: &TagKnowledge<'a>,
    ) -> Option<String> {
        let mut candidates: Vec<String> = Vec::new();
        match knowledge {
            TagKnowledge::Components(comps) => {
                candidates.extend(
                    comps
                        .iter()
                        .flat_map(|c| c.attributes.iter())
                        .map(|a| a.name.clone()),
                );
            }
            TagKnowledge::Custom(tag) => {
                candidates.extend(tag.attributes.iter().map(|a| a.name.clone()));
            }
            TagKnowledge::Builtin(data) => {
                candidates.extend(data.attributes.iter().map(|a| a.name.to_string()));
                if matches!(el.kind, ElementKind::Svg | ElementKind::MathMl) {
                    candidates.extend(
                        fast_html_data::SVG_PRESENTATION_ATTRIBUTES
                            .iter()
                            .map(|a| a.name.to_string()),
                    );
                }
            }
            _ => {}
        }
        candidates.extend(
            fast_html_data::GLOBAL_ATTRIBUTES
                .iter()
                .map(|a| a.name.to_string()),
        );
        suggest::nearest(name, candidates.iter().map(String::as_str)).map(str::to_string)
    }

    fn suggest_property(&self, name: &str, knowledge: &TagKnowledge<'a>) -> Option<String> {
        let mut candidates: Vec<String> = Vec::new();
        if let TagKnowledge::Components(comps) = knowledge {
            candidates.extend(comps.iter().flat_map(|c| c.properties.iter()).map(|p| p.name.clone()));
            candidates.extend(
                comps
                    .iter()
                    .flat_map(|c| c.attributes.iter())
                    .filter_map(|a| a.property_name.clone()),
            );
        }
        suggest::nearest(name, candidates.iter().map(String::as_str)).map(str::to_string)
    }

    fn suggest_event(&self, name: &str, knowledge: &TagKnowledge<'a>) -> Option<String> {
        let mut candidates: Vec<String> =
            fast_html_data::events().map(|(n, _)| n.to_string()).collect();
        if let TagKnowledge::Components(comps) = knowledge {
            candidates.extend(comps.iter().flat_map(|c| c.events.iter()).map(|e| e.name.clone()));
        }
        suggest::nearest(name, candidates.iter().map(String::as_str)).map(str::to_string)
    }
}
