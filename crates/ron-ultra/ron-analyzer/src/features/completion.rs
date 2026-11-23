//! Completion from the Rust type a file is annotated with.
//!
//! Three places offer something:
//!
//! - inside `@[...]`: crates, modules and types, segment by segment;
//! - where a struct body expects a key: the fields not yet written;
//! - where a value goes: what its type accepts — variants, `Some`/`None`,
//!   `true`/`false`, a struct skeleton with its required fields.
//!
//! The expected type at the cursor comes from the checker's [`BodyFact`]s,
//! so completion agrees with diagnostics about renames, flattening and
//! extensions.

use crate::ast::{Container, EntryKey, Value};
use crate::lexer::{lex, TokenKind};
use crate::rust::syntax::{Fields, ItemKind};
use crate::rust::world::{Res, World};
use crate::schema::check::{field_set, variant_name, Body, BodyFact, Exts, FieldSet};
use crate::schema::ty::{Lowerer, Ty};
use crate::schema::Schema;
use crate::workspace::ParsedFile;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    Field,
    Variant,
    Struct,
    Enum,
    Module,
    Keyword,
    Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionItem {
    pub label: String,
    pub kind: CompletionKind,
    pub detail: Option<String>,
    /// Markdown.
    pub documentation: Option<String>,
    /// What to insert; the label when `None`.
    pub insert_text: Option<String>,
    /// `insert_text` is an LSP snippet.
    pub snippet: bool,
    pub sort_text: Option<String>,
    /// Ask for completion again after inserting, as after `field: `.
    pub retrigger: bool,
}

impl CompletionItem {
    fn new(label: impl Into<String>, kind: CompletionKind) -> Self {
        CompletionItem {
            label: label.into(),
            kind,
            detail: None,
            documentation: None,
            insert_text: None,
            snippet: false,
            sort_text: None,
            retrigger: false,
        }
    }

    fn snippet(mut self, text: impl Into<String>) -> Self {
        self.insert_text = Some(text.into());
        self.snippet = true;
        self
    }

    fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    fn docs(mut self, docs: &str) -> Self {
        if !docs.is_empty() {
            self.documentation = Some(docs.to_string());
        }
        self
    }
}

pub fn completions(pf: &ParsedFile, schema: &Schema, world: &World, offset: u32) -> Vec<CompletionItem> {
    if let Some(ann) = &schema.annotation {
        if ann.span.start <= offset && offset <= ann.end {
            return annotation_items(pf, world, &pf.source[ann.span.start as usize..offset as usize]);
        }
    }
    let Some(root) = &schema.root else { return Vec::new() };
    let low = Lowerer::new(world);
    let tokens: Vec<_> = lex(&pf.source).tokens;
    // Nothing inside a comment, a string or a number.
    if tokens.iter().any(|t| match t.kind {
        // A line comment runs to the end of its line, cursor included.
        TokenKind::LineComment => t.span.start < offset && offset <= t.span.end,
        TokenKind::BlockComment | TokenKind::String | TokenKind::Char | TokenKind::ByteString => {
            t.span.start < offset && offset < t.span.end
        }
        _ => false,
    }) {
        return Vec::new();
    }
    let significant: Vec<_> = tokens.iter().filter(|t| !t.kind.is_comment()).collect();
    // The token before the word being typed.
    let mut before = significant.iter().rev().filter(|t| t.span.start < offset);
    let mut prev = before.next();
    if prev.is_some_and(|t| t.kind == TokenKind::Ident && t.span.end >= offset) {
        prev = before.next();
    }
    let prev_kind = prev.map(|t| t.kind);

    let Some(body) = innermost_body(&schema.facts.bodies, offset) else {
        // Top level: the root value itself, being typed or not yet there.
        let at_root = match &pf.ast.value {
            None => true,
            Some(v) => v.span.contains(offset) && v.container().is_none(),
        };
        return if at_root { value_items(&low, root, schema.exts) } else { Vec::new() };
    };
    let container = container_at(pf.ast.value.as_ref(), body.span.start);

    match (prev_kind, &body.body) {
        (Some(TokenKind::Colon), Body::Fields(set)) => {
            let key = prev.and_then(|colon| {
                significant.iter().rev().find(|t| t.span.end <= colon.span.start).filter(|t| t.kind == TokenKind::Ident)
            });
            let Some(key) = key else { return Vec::new() };
            let name = &pf.source[key.span.start as usize..key.span.end as usize];
            let name = name.strip_prefix("r#").unwrap_or(name);
            match set.fields.iter().find(|f| f.name == name || f.aliases.iter().any(|a| a == name)) {
                Some(f) => value_items(&low, &f.ty, schema.exts),
                None => set.open.as_ref().map_or_else(Vec::new, |t| value_items(&low, t, schema.exts)),
            }
        }
        (Some(TokenKind::Colon), Body::Map(_, v)) => value_items(&low, v, schema.exts),
        (Some(TokenKind::Comma | TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace), body_kind) => {
            match body_kind {
                Body::Fields(set) => key_items(set, container),
                Body::Positional(tys) => {
                    let index = container.map_or(0, |c| c.entries.iter().filter(|e| e.span().end < offset).count());
                    tys.get(index).map_or_else(Vec::new, |t| value_items(&low, t, schema.exts))
                }
                Body::Elements(t) => value_items(&low, t, schema.exts),
                Body::Map(k, _) => value_items(&low, k, schema.exts),
            }
        }
        _ => Vec::new(),
    }
}

fn innermost_body(bodies: &[BodyFact], offset: u32) -> Option<&BodyFact> {
    bodies
        .iter()
        // After the opening bracket, and before the closing one (or anywhere
        // to the end of the file when it is missing).
        .filter(|b| b.span.start < offset && (offset < b.span.end || (!b.closed && offset == b.span.end)))
        .min_by_key(|b| b.span.end - b.span.start)
}

/// The container whose opening bracket starts at `open`.
fn container_at(value: Option<&Value>, open: u32) -> Option<&Container> {
    let v = value?;
    let c = v.container()?;
    if c.open.start == open {
        return Some(c);
    }
    c.entries.iter().find_map(|e| {
        let key = match &e.key {
            Some(EntryKey::Map(k)) => container_at(Some(k), open),
            _ => None,
        };
        key.or_else(|| container_at(Some(&e.value), open))
    })
}

fn key_items(set: &FieldSet, container: Option<&Container>) -> Vec<CompletionItem> {
    let present: Vec<&str> = container
        .map(|c| {
            c.entries
                .iter()
                .filter_map(|e| match &e.key {
                    Some(EntryKey::Field(id)) => Some(id.name.as_str()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    set.fields
        .iter()
        .filter(|f| !present.contains(&f.name.as_str()) && !f.aliases.iter().any(|a| present.contains(&a.as_str())))
        .map(|f| {
            let field = f.field();
            let mut item = CompletionItem::new(f.name.clone(), CompletionKind::Field)
                .detail(field.ty_text.clone())
                .docs(&field.docs);
            item.insert_text = Some(format!("{}: ", f.name));
            item.sort_text = Some(format!("{}{}", if f.required { 0 } else { 1 }, f.name));
            item.retrigger = true;
            item
        })
        .collect()
}

/// What a value of type `ty` can be written as.
fn value_items(low: &Lowerer, ty: &Ty, exts: Exts) -> Vec<CompletionItem> {
    let mut out = Vec::new();
    push_values(low, ty, exts, &mut out, 0);
    out
}

fn push_values(low: &Lowerer, ty: &Ty, exts: Exts, out: &mut Vec<CompletionItem>, depth: u32) {
    if depth > 4 {
        return;
    }
    match ty {
        Ty::Bool => {
            out.push(CompletionItem::new("true", CompletionKind::Keyword));
            out.push(CompletionItem::new("false", CompletionKind::Keyword));
        }
        Ty::Option(inner) => {
            out.push(CompletionItem::new("None", CompletionKind::Keyword).detail(ty.to_string()));
            out.push(CompletionItem::new("Some(…)", CompletionKind::Keyword).detail(ty.to_string()).snippet("Some($1)"));
            if exts.implicit_some {
                push_values(low, inner, exts, out, depth + 1);
            }
        }
        Ty::Unit => out.push(CompletionItem::new("()", CompletionKind::Value)),
        Ty::Str => out.push(CompletionItem::new("\"\"", CompletionKind::Value).detail(ty.to_string()).snippet("\"$1\"")),
        Ty::Seq(..) | Ty::Array(..) => {
            out.push(CompletionItem::new("[…]", CompletionKind::Value).detail(ty.to_string()).snippet("[$1]"));
        }
        Ty::Map(..) => out.push(CompletionItem::new("{…}", CompletionKind::Value).detail(ty.to_string()).snippet("{$1}")),
        Ty::Tuple(ts) => {
            let stops: Vec<String> = (1..=ts.len()).map(|i| format!("${i}")).collect();
            out.push(
                CompletionItem::new("(…)", CompletionKind::Value)
                    .detail(ty.to_string())
                    .snippet(format!("({})", stops.join(", "))),
            );
        }
        Ty::Adt(adt) => {
            if let Some(from) = low.from_ty(adt) {
                return push_values(low, &from, exts, out, depth + 1);
            }
            let item = adt.item.item();
            match &item.kind {
                ItemKind::Struct { fields, .. } => {
                    if item.serde.transparent {
                        if let Some(f) = fields.list().iter().find(|f| !f.serde.skip) {
                            push_values(low, &low.field_ty(adt, f), exts, out, depth + 1);
                        }
                        return;
                    }
                    let name = item.serde.rename.clone().unwrap_or_else(|| item.name().to_string());
                    let snippet = match fields {
                        Fields::Named(fs) => {
                            let set = field_set(
                                low,
                                adt,
                                None,
                                fs,
                                item.serde.rename_all.as_deref(),
                                item.serde.default,
                                false,
                                &name,
                            );
                            format!("{name}({})", fields_snippet(&set))
                        }
                        Fields::Tuple(fs) if fs.len() == 1 && exts.unwrap_newtypes => {
                            return push_values(low, &low.field_ty(adt, &fs[0]), exts, out, depth + 1);
                        }
                        Fields::Tuple(fs) => format!("{name}({})", positional_snippet(fs.len())),
                        Fields::Unit => name.clone(),
                    };
                    let label = if matches!(fields, Fields::Unit) { name.clone() } else { format!("{name}(…)") };
                    out.push(
                        CompletionItem::new(label, CompletionKind::Struct)
                            .detail(format!("struct {}", item.name()))
                            .docs(&item.docs)
                            .snippet(snippet),
                    );
                }
                ItemKind::Enum { variants, .. } => {
                    if item.serde.untagged {
                        return;
                    }
                    for (i, var) in variants.iter().enumerate().filter(|(_, v)| !v.serde.skip) {
                        let name = variant_name(adt, var);
                        let label_full = format!("{}::{}", item.name(), var.name.name);
                        let snippet = if let Some(tag) = &item.serde.tag {
                            match &item.serde.content {
                                Some(content) if !matches!(var.fields, Fields::Unit) => {
                                    format!("({tag}: \"{name}\", {content}: $1)")
                                }
                                _ => format!("({tag}: \"{name}\"$1)"),
                            }
                        } else {
                            match &var.fields {
                                Fields::Unit => name.clone(),
                                Fields::Tuple(fs) => format!("{name}({})", positional_snippet(fs.len())),
                                Fields::Named(fs) => {
                                    let rule = var.serde.rename_all.as_deref().or(item.serde.rename_all_fields.as_deref());
                                    let set = field_set(low, adt, Some(i), fs, rule, false, false, &label_full);
                                    format!("{name}({})", fields_snippet(&set))
                                }
                            }
                        };
                        let label = if matches!(var.fields, Fields::Unit) || item.serde.tag.is_some() {
                            name.clone()
                        } else {
                            format!("{name}(…)")
                        };
                        let mut ci = CompletionItem::new(label, CompletionKind::Variant).detail(label_full).docs(&var.docs);
                        if snippet != name {
                            ci = ci.snippet(snippet);
                        }
                        ci.sort_text = Some(format!("{i:04}"));
                        out.push(ci);
                    }
                }
                _ => {}
            }
        }
        Ty::Any | Ty::Int(_) | Ty::Float(_) | Ty::Char => {}
    }
}

/// `\n\tname: $1,\n\tother: $2,\n` for the required fields, or `$1` when
/// there are none.
fn fields_snippet(set: &FieldSet) -> String {
    let required: Vec<&str> = set.fields.iter().filter(|f| f.required).map(|f| f.name.as_str()).collect();
    if required.is_empty() {
        return "$1".into();
    }
    let mut s = String::from("\n");
    for (i, name) in required.iter().enumerate() {
        s.push_str(&format!("\t{name}: ${},\n", i + 1));
    }
    s
}

fn positional_snippet(n: usize) -> String {
    (1..=n.max(1)).map(|i| format!("${i}")).collect::<Vec<_>>().join(", ")
}

/// Completing the path inside `@[...]`; `typed` is the annotation text up
/// to the cursor.
fn annotation_items(pf: &ParsedFile, world: &World, typed: &str) -> Vec<CompletionItem> {
    // The path being typed: the run of path characters before the cursor,
    // so `Vec<crate::con` completes `crate::con`.
    let start = typed.rfind(|c: char| !(c.is_alphanumeric() || c == '_' || c == ':')).map_or(0, |i| i + 1);
    let path = &typed[start..];
    let mut segments: Vec<&str> = path.split("::").collect();
    segments.pop(); // the partial segment
    let crates = world.crates_for(pf.uri.as_str());
    let Some(krate) = crates.first() else { return Vec::new() };
    let mut out = Vec::new();
    let push_names = |scope: &crate::rust::world::Scope, out: &mut Vec<CompletionItem>| {
        for (name, res) in world.names_in(scope) {
            if out.iter().any(|c: &CompletionItem| c.label == name) {
                continue;
            }
            let item = match res {
                Res::Module(m) => CompletionItem::new(name, CompletionKind::Module).detail(format!("mod {}", m.display(Some(krate)))),
                Res::Item(r) => {
                    let def = r.item();
                    let kind = match def.kind {
                        ItemKind::Enum { .. } => CompletionKind::Enum,
                        _ => CompletionKind::Struct,
                    };
                    CompletionItem::new(name, kind)
                        .detail(format!("{} {}::{}", def.keyword(), r.module.display(Some(krate)), def.name()))
                        .docs(&def.docs)
                }
                Res::External(_) => continue,
            };
            out.push(item);
        }
    };
    if segments.is_empty() {
        out.push(CompletionItem::new("crate", CompletionKind::Keyword).detail(format!("crate {}", krate.name)));
        for dep in world.dependency_names(krate) {
            out.push(CompletionItem::new(dep.clone(), CompletionKind::Module).detail(format!("extern crate {dep}")));
        }
        if let Some(scope) = world.root_scope(krate.clone()) {
            push_names(&scope, &mut out);
        }
        return out;
    }
    let names: Vec<String> = segments.iter().map(|s| s.to_string()).collect();
    for k in &crates {
        let Some(root) = world.root_scope(k.clone()) else { continue };
        if let Some(Res::Module(m)) = world.resolve_path(&root, false, &names) {
            if let Some(scope) = world.scope(&m) {
                push_names(&scope, &mut out);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::tests::analyzed;

    fn labels_at(src: &str, marker: &str) -> Vec<String> {
        let offset = src.find(marker).expect("marker") as u32;
        let src = src.replacen(marker, "", 1);
        let (pf, schema, world) = analyzed(&src);
        completions(&pf, &schema, &world, offset).into_iter().map(|c| c.label).collect()
    }

    const HEAD: &str = "/* @[crate::config::GameConfig] */\n";

    #[test]
    fn missing_fields_at_key_positions() {
        let src = format!("{HEAD}GameConfig(\n    window_title: \"x\",\n    |\n)");
        let labels = labels_at(&src, "|");
        assert!(labels.contains(&"window_size".to_string()), "{labels:?}");
        assert!(labels.contains(&"fullscreen".to_string()));
        assert!(!labels.contains(&"window_title".to_string()));
        assert!(!labels.contains(&"runtime".to_string()), "skipped fields are not offered");
        // Half-typed key.
        let src = format!("{HEAD}GameConfig(window_title: \"x\", win|)");
        assert!(labels_at(&src, "|").contains(&"window_size".to_string()));
    }

    #[test]
    fn values_by_type() {
        let src = format!("{HEAD}GameConfig(fullscreen: |)");
        assert_eq!(labels_at(&src, "|"), ["true", "false"]);
        let src = format!("{HEAD}GameConfig(difficulty: (start: E|))");
        assert_eq!(labels_at(&src, "|"), ["Easy", "Hard"]);
        let src = format!("{HEAD}GameConfig(save_slot: |)");
        assert_eq!(labels_at(&src, "|"), ["None", "Some(…)"]);
        let src = format!("{HEAD}GameConfig(shapes: [Circle(1.0), |])");
        assert_eq!(labels_at(&src, "|"), ["Circle(…)", "Rect(…)", "Point(…)"]);
        let src = format!("{HEAD}GameConfig(key_bindings: {{\"up\": |}})");
        assert_eq!(labels_at(&src, "|"), ["move_up", "move_down", "fire"]);
        let src = format!("{HEAD}GameConfig(difficulty: |)");
        assert_eq!(labels_at(&src, "|"), ["Difficulty(…)"]);
    }

    #[test]
    fn root_value() {
        assert_eq!(labels_at(&format!("{HEAD}|"), "|"), ["GameConfig(…)"]);
        assert_eq!(labels_at(&format!("{HEAD}Game|"), "|"), ["GameConfig(…)"]);
    }

    #[test]
    fn struct_skeleton_lists_required_fields() {
        let src = format!("{HEAD}GameConfig(difficulty: |)");
        let offset = src.find('|').unwrap() as u32;
        let (pf, schema, world) = analyzed(&src.replace('|', ""));
        let items = completions(&pf, &schema, &world, offset);
        assert_eq!(items[0].insert_text.as_deref(), Some("Difficulty(\n\tstart: $1,\n\tadaptive: $2,\n)"));
    }

    #[test]
    fn nothing_in_strings_or_comments() {
        assert!(labels_at(&format!("{HEAD}GameConfig(window_title: \"a|b\")"), "|").is_empty());
        assert!(labels_at(&format!("{HEAD}GameConfig(\n// note |\n)"), "|").is_empty());
    }

    #[test]
    fn annotation_paths() {
        let labels = labels_at("/* @[cr|] */\n()", "|");
        assert!(labels.contains(&"crate".to_string()), "{labels:?}");
        assert!(labels.contains(&"config".to_string()));
        let labels = labels_at("/* @[crate::config::|] */\n()", "|");
        assert!(labels.contains(&"GameConfig".to_string()), "{labels:?}");
        assert!(labels.contains(&"Level".to_string()));
        // Still typing: no closing bracket yet.
        let labels = labels_at("/* @[crate::|", "|");
        assert_eq!(labels, ["config"]);
    }
}
