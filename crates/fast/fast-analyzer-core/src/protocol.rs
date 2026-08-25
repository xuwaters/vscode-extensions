//! The boundary protocol: every type that crosses between the TypeScript
//! plugin and this engine, as JSON via serde. The TypeScript mirror lives in
//! `extensions/fast-element-ultra/tsplugin/protocol.ts`; the two are kept in
//! step by hand and exercised by the extension's integration tests.
//!
//! Offsets: every `start`/`end` that names a position in a *source file* or a
//! *virtual document* is in UTF-16 code units — JavaScript's coordinate
//! system, so the plugin never converts. The engine converts to and from byte
//! offsets internally (`documents.rs`).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// An absolute location in some source file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSpan {
    pub file_name: String,
    pub start: u32,
    pub end: u32,
}

// ------------------------------------------------------------------- config

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    pub strict: bool,
    /// Rule id → `"default" | "off" | "warning" | "error"`.
    pub rules: HashMap<String, String>,
    pub global_tags: Vec<String>,
    pub global_attributes: Vec<String>,
    pub global_events: Vec<String>,
    pub dont_show_suggestions: bool,
    /// Parsed VS Code custom-data documents, already resolved from paths by
    /// the plugin — the engine does no I/O.
    pub custom_html_data: Vec<serde_json::Value>,
    /// Import depth `no-missing-import` searches, -1 = unlimited.
    pub max_project_import_depth: i32,
    pub max_node_module_import_depth: i32,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            strict: false,
            rules: HashMap::new(),
            global_tags: Vec::new(),
            global_attributes: Vec::new(),
            global_events: Vec::new(),
            dont_show_suggestions: false,
            custom_html_data: Vec::new(),
            max_project_import_depth: -1,
            max_node_module_import_depth: 1,
        }
    }
}

// -------------------------------------------------------------- registry in

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MemberFact {
    /// The attribute name for attributes; the property name for properties.
    pub name: String,
    /// For an attribute: the property it reflects.
    pub property_name: Option<String>,
    /// For an attribute: `reflect` | `boolean` | `fromView`.
    pub mode: Option<String>,
    pub type_text: Option<String>,
    pub type_id: Option<u32>,
    pub declaration_id: Option<u32>,
    pub decl_span: Option<FileSpan>,
    pub documentation: Option<String>,
    /// `decorator` | `definition` | `jsdoc` | `inherited`.
    pub origin: String,
    /// `public` | `protected` | `private`.
    pub visibility: Option<String>,
    /// Enumerated values, extracted from a union of string literals — feeds
    /// attribute-value completion.
    pub values: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EventFact {
    pub name: String,
    pub type_text: Option<String>,
    pub decl_span: Option<FileSpan>,
    pub documentation: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NamedFact {
    pub name: String,
    pub documentation: Option<String>,
    pub decl_span: Option<FileSpan>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ComponentFact {
    /// `None` when the definition's `name` is not a string-literal type: the
    /// component still registers — members are still useful — but it is
    /// excluded from tag lookups.
    pub tag_name: Option<String>,
    pub class_name: String,
    /// The contents of the tag-name string literal (or the `const` it resolves
    /// through), for rename.
    pub tag_name_span: Option<FileSpan>,
    /// The class-name identifier.
    pub decl_span: Option<FileSpan>,
    pub declaration_id: Option<u32>,
    /// Interned id of the class instance type, for matching `html<T>`.
    pub source_type_id: Option<u32>,
    pub attributes: Vec<MemberFact>,
    pub properties: Vec<MemberFact>,
    pub events: Vec<EventFact>,
    pub slots: Vec<NamedFact>,
    pub css_parts: Vec<NamedFact>,
    pub css_properties: Vec<NamedFact>,
    pub has_shadow_root: bool,
    /// The virtual document named as `template:` in the definition.
    pub template_document_id: Option<String>,
    /// Ids of `css` documents named as `styles:` in the definition.
    pub style_document_ids: Vec<String>,
    pub documentation: Option<String>,
    pub in_tag_name_map: bool,
    /// `decorator` | `define`.
    pub origin: String,
}

// ------------------------------------------------------------- documents in

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SourceMember {
    pub name: String,
    pub type_text: Option<String>,
    pub documentation: Option<String>,
    pub decl_span: Option<FileSpan>,
    pub is_function: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DirectiveInfo {
    /// `when` | `repeat` | `render` | `ref` | `slotted` | `children`.
    pub name: String,
    /// The string-literal argument of `ref`/`slotted`/`children`.
    pub arg_string: Option<String>,
    /// Document-relative span of the argument's contents, when it is a string
    /// literal inside the placeholder run.
    pub arg_start: Option<u32>,
    pub arg_end: Option<u32>,
}

/// What the plugin's AST walk learned about one `${…}` expression. Everything
/// here is compiler knowledge computed once at upsert; the engine never asks
/// follow-up questions about an expression except through binding facts.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ExprInfo {
    /// `arrow` | `function` | `call` | `identifier` | `propertyAccess` |
    /// `literal` | `template` | `other`.
    pub kind: String,
    /// Does the expression's type have call signatures — i.e. is this a
    /// reactive binding?
    pub is_function_type: Option<bool>,
    /// A literal type or a `const`-declared symbol: a deliberate one-time
    /// binding, exempt from `no-non-reactive-binding`.
    pub is_constant: Option<bool>,
    /// Typed as a FAST directive, `Binding`, or `ViewTemplate` — used as
    /// given by the runtime, never a mistake.
    pub is_directive_value: bool,
    pub directive: Option<DirectiveInfo>,
    /// `html.partial(…)`: analysis of the whole document stops.
    pub is_partial: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PlaceholderFact {
    pub index: u32,
    /// Document-relative, UTF-16, covering `${` through `}`.
    pub start: u32,
    pub end: u32,
    pub expr: Option<ExprInfo>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VirtualDocumentFact {
    /// `fileName#templateStart` — stable across edits that do not move it.
    pub id: String,
    pub file_name: String,
    /// Source-file offset of the first character after the backtick.
    pub template_start: u32,
    /// `html` | `css`.
    pub kind: String,
    /// The substituted text, length-preserved.
    pub text: String,
    pub placeholders: Vec<PlaceholderFact>,
    pub source_type_id: Option<u32>,
    pub parent_type_id: Option<u32>,
    pub source_type_name: Option<String>,
    /// The properties of `TSource`, when it is known.
    pub source_members: Option<Vec<SourceMember>>,
    /// The tag of the component whose `template:` (or `styles:`) this is.
    pub component_tag: Option<String>,
    /// Absolute offset right after the tag identifier, where `<T>` would be
    /// inserted by the `no-untyped-template` fix.
    pub type_arg_insert_offset: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UpsertFile {
    pub file_name: String,
    /// Resolved file names this module imports, in-project.
    pub dependencies: Vec<String>,
    /// Resolved file names this module imports from node_modules.
    pub node_module_dependencies: Vec<String>,
    pub components: Vec<ComponentFact>,
    pub documents: Vec<VirtualDocumentFact>,
    /// Events this file added to `HTMLElementEventMap`. Sent only for the
    /// synthetic ambient file, where the merged map is read once per program.
    pub global_events: Vec<EventFact>,
}

// ------------------------------------------------------------ analysis out

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    Warning,
    Error,
    Suggestion,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Edit {
    /// `None`: document-relative; the plugin adds `templateStart`.
    /// `Some`: an absolute location in that file.
    pub file_name: Option<String>,
    pub start: u32,
    pub end: u32,
    pub new_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FixCommand {
    /// `addImport`: the plugin synthesizes the import edit.
    pub kind: String,
    pub target_file: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Fix {
    pub label: String,
    pub edits: Vec<Edit>,
    pub command: Option<FixCommand>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub rule_id: String,
    pub severity: Severity,
    pub message: String,
    /// Document-relative, UTF-16.
    pub start: u32,
    pub end: u32,
    /// Why the engine believes the tag exists: `declaration` | `customData` |
    /// `globalTags` | `builtin`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub fixes: Vec<Fix>,
}

/// A type question the engine cannot answer. One is emitted per binding that
/// reaches a type rule; the plugin answers the batch after the pass.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BindingFact {
    /// `attribute` | `booleanAttribute` | `property` | `event`.
    pub kind: String,
    /// Document-relative span to report any resulting diagnostic at.
    pub start: u32,
    pub end: u32,
    pub tag_name: String,
    /// The attribute/property/event name being bound.
    pub member_name: Option<String>,
    /// The declaration of the target member, when the registry resolved one.
    pub target_declaration_id: Option<u32>,
    /// `attribute` | `property` | `event` — which table resolved it.
    pub target_kind: Option<String>,
    /// The tag is a built-in element: the plugin resolves the member type
    /// from the DOM lib instead.
    pub target_builtin: bool,
    pub expression_index: Option<u32>,
    /// The literal value, when the binding has no expression.
    pub literal: Option<String>,
    /// For attribute facts: the target's declared `mode`.
    pub mode: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AnalyzeResult {
    pub diagnostics: Vec<Diagnostic>,
    pub facts: Vec<BindingFact>,
}

// ---------------------------------------------------------------- queries

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Query {
    Completions {
        document_id: String,
        offset: u32,
    },
    QuickInfo {
        document_id: String,
        offset: u32,
    },
    Definition {
        document_id: String,
        offset: u32,
    },
    References {
        document_id: String,
        offset: u32,
    },
    /// References to a member from its declaration side — the plugin resolved
    /// which component and member the cursor is on.
    MemberReferences {
        tag: Option<String>,
        source_type_id: Option<u32>,
        name: String,
    },
    TagReferences {
        tag: String,
    },
    RenameInfo {
        document_id: String,
        offset: u32,
    },
    RenameLocations {
        document_id: String,
        offset: u32,
    },
    MemberRenameLocations {
        tag: Option<String>,
        source_type_id: Option<u32>,
        name: String,
    },
    TagRenameLocations {
        tag: String,
    },
    ClosingTag {
        document_id: String,
        offset: u32,
    },
    Folding {
        document_id: String,
    },
    CodeFixes {
        document_id: String,
        start: u32,
        end: u32,
    },
    DocumentInfoAt {
        document_id: String,
        offset: u32,
    },
    Severities,
    FileDiagnostics {
        file_name: String,
    },
    /// The parsed tree in a comparable shape — the parse5 differential
    /// harness's window into the parser (research/spikes.md, gate 2).
    ParseTree {
        document_id: String,
    },
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TreeNode {
    pub kind: String,
    /// Element name, or the text/comment contents for leaves.
    pub name: String,
    /// Attribute names (modifier included), lowercased, sorted.
    pub attrs: Vec<String>,
    pub element_expressions: u32,
    pub closed: bool,
    pub implied: bool,
    pub self_closing: bool,
    pub children: Vec<TreeNode>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CompletionItem {
    pub name: String,
    /// `tag` | `attribute` | `property` | `event` | `booleanAttribute` |
    /// `value` | `slotName` | `part` | `snippet` | `member`.
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub insert_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documentation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_text: Option<String>,
    /// Set when picking this item should also add an import of that file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub import_from: Option<String>,
    pub is_snippet: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Completions {
    pub items: Vec<CompletionItem>,
    /// Document-relative range the items replace; `None` = insert at cursor.
    pub replace_start: Option<u32>,
    pub replace_end: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct QuickInfo {
    /// Markdown.
    pub contents: String,
    pub start: u32,
    pub end: u32,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DefinitionResult {
    pub targets: Vec<FileSpan>,
    pub origin_start: u32,
    pub origin_end: u32,
    pub name: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RenameInfo {
    pub can_rename: bool,
    pub display_name: String,
    pub trigger_start: u32,
    pub trigger_end: u32,
    /// `tag` | `member` — echoed back in the locations query.
    pub kind: String,
    pub tag: Option<String>,
    pub member: Option<String>,
    pub source_type_id: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ClosingTagResult {
    pub new_text: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FoldingRange {
    pub start: u32,
    pub end: u32,
}

/// A diagnostic against a source file rather than a virtual document —
/// registry-level rules (`no-duplicate-tag-name`, `no-invalid-tag-name`)
/// whose spans point at registrations, not templates.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDiagnostic {
    pub rule_id: String,
    pub severity: Severity,
    pub message: String,
    pub file_name: String,
    pub start: u32,
    pub end: u32,
}

/// Position context the plugin uses to route features it computes itself —
/// `ref('…')` completions above all, which live inside a placeholder where
/// the engine sees only underscores.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DocumentInfoAt {
    pub in_placeholder: Option<u32>,
    /// The directive whose string argument contains the offset.
    pub directive_name: Option<String>,
    pub directive_arg_start: Option<u32>,
    pub directive_arg_end: Option<u32>,
    pub in_template: bool,
}
