//! Named arguments a function accepts but does not declare.
//!
//! A Typst function can take its real arguments through a sink and read them
//! back out of a dictionary:
//!
//! ```text
//! #let circle(..points-style, name: none, anchor: none) = {
//!   let style = styles.resolve(ctx.style, merge: points-style.named(), root: "circle")
//!   ..
//! }
//! ```
//!
//! `circle((0, 0), radius: 2, fill: red)` is the way that function is meant to
//! be called, and `Func::params()` — which is all upstream completion looks at
//! — knows only `name` and `anchor`. The accepted names are written down in
//! two other places, and this module reads both:
//!
//! * the doc comment's `Styling` section, parsed by [`crate::docs`]; and
//! * the style dictionary the section's `*Root*` names, which libraries keep
//!   as a `default` dictionary keyed by root — `(circle: (radius: auto,
//!   stroke: auto, fill: auto), ..)`. The docs list the keys that are
//!   *interesting*; the dictionary has the ones that merely work.
//!
//! Both are conventions, not language features, so everything here fails soft:
//! a function that follows neither gets no extra completions and nothing else
//! changes.

use ecow::EcoString;
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionTextEdit, Documentation, InsertTextFormat,
    MarkupContent, MarkupKind, TextEdit,
};
use typst::World;
use typst::foundations::{Func, Repr, Value};
use typst::syntax::ast::AstNode;
use typst::syntax::{FileId, LinkedNode, Side, Source, SyntaxKind, ast};

use crate::convert::range_to_lsp;
use crate::docs::{DocComment, DocEntry};
use crate::{Ports, Server};

/// A cursor sitting where a named argument may be written.
struct ParamContext<'a> {
    /// The node naming the function being called.
    callee: LinkedNode<'a>,
    /// Where a completion's replacement starts, following upstream's rule so
    /// our items edit exactly like its own.
    from: usize,
    /// The named arguments already written in this call.
    named: Vec<EcoString>,
}

impl<Q: Ports> Server<Q> {
    /// Document the named arguments of the call the cursor is inside.
    ///
    /// Enriches the items upstream produced for the function's declared
    /// parameters with the documentation it could not find, and appends the
    /// arguments the function accepts through its sink. Does nothing at all
    /// away from an argument list.
    pub(crate) fn document_named_arguments(
        &self,
        source: &Source,
        cursor: usize,
        explicit: bool,
        items: &mut Vec<CompletionItem>,
    ) {
        let Some(context) = param_context(source, cursor, explicit) else {
            return;
        };
        let Some(func) = self.func_at(&context.callee) else {
            return;
        };
        let Some(docs) = self.doc_comment_of(&func) else {
            return;
        };

        enrich_param_items(items, &docs);

        // A function that declares every argument it takes has nothing hidden
        // to offer; leave it to upstream. A closure's sink is reported as a
        // positional variadic even though `..sink` swallows named arguments
        // too — `sink.named()` inside the body is how they are read back.
        if !func.params().any(|param| param.variadic()) {
            return;
        }

        items.extend(self.sink_completions(source, cursor, &context, &func, &docs, items));
    }

    /// The items for arguments accepted through a sink.
    fn sink_completions(
        &self,
        source: &Source,
        cursor: usize,
        context: &ParamContext,
        func: &Func,
        docs: &DocComment,
        existing: &[CompletionItem],
    ) -> Vec<CompletionItem> {
        let keys = self.hidden_named_arguments(func, docs);
        if keys.is_empty() {
            return Vec::new();
        }

        let range = range_to_lsp(source, context.from..cursor);
        let declared: Vec<String> = func
            .params()
            .filter_map(|param| param.name().map(str::to_string))
            .collect();

        keys.into_iter()
            .filter(|(name, _)| !context.named.iter().any(|written| written == name))
            .filter(|(name, _)| !declared.iter().any(|known| known == name.as_str()))
            .filter(|(name, _)| {
                !existing
                    .iter()
                    .any(|item| item.label.as_str() == name.as_str())
            })
            .enumerate()
            .map(|(index, (name, entry))| CompletionItem {
                label: name.to_string(),
                kind: Some(CompletionItemKind::FIELD),
                detail: entry.as_ref().map(|entry| entry.signature().to_string()),
                documentation: entry
                    .as_ref()
                    .filter(|entry| !entry.docs.is_empty())
                    .map(|entry| {
                        Documentation::MarkupContent(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: entry.docs.to_string(),
                        })
                    }),
                text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                    range,
                    new_text: format!("{name}: $1"),
                })),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                // After upstream's own parameters, in the order the library
                // documents them.
                sort_text: Some(format!("{:06}", 500_000 + index)),
                ..CompletionItem::default()
            })
            .collect()
    }

    /// A hover answered from a doc comment, where there is a richer one than
    /// upstream's first sentence.
    ///
    /// Two cursors qualify: the name of a function that carries a doc comment,
    /// and the name of an argument that comment documents — including the ones
    /// the signature never declares.
    pub(crate) fn doc_comment_hover(&self, source: &Source, cursor: usize) -> Option<EcoString> {
        let root = LinkedNode::new(source.root());
        let leaf = root
            .leaf_at(cursor, Side::Before)
            .or_else(|| root.leaf_at(cursor, Side::After))?;
        if leaf.kind() != SyntaxKind::Ident {
            return None;
        }

        // `f(name: |)` — the argument, not the function.
        if leaf.parent_kind() == Some(SyntaxKind::Named) {
            let named = leaf.parent()?;
            let args = named.parent()?;
            let call = args.parent()?.get().cast::<ast::FuncCall>()?;
            let callee = args.parent()?.find(call.callee().span())?;

            let func = self.func_at(&callee)?;
            let docs = self.doc_comment_of(&func)?;
            let name = leaf.get().cast::<ast::Ident>()?;
            let entry = docs.param(name.as_str())?;

            let mut out = EcoString::from("```typst\n");
            out.push_str(&entry.signature());
            out.push_str("\n```");
            if !entry.docs.is_empty() {
                out.push_str("\n\n");
                out.push_str(&entry.docs);
            }
            return Some(out);
        }

        let func = self.func_at(&leaf)?;
        Some(self.doc_comment_of(&func)?.markdown())
    }

    /// The doc comment written above a function's definition.
    ///
    /// Only closures have one: a native function carries its documentation in
    /// the binary.
    pub(crate) fn doc_comment_of(&self, func: &Func) -> Option<DocComment> {
        let span = func.span();
        let id = span.id()?;
        let source = self.session().world().source(id).ok()?;
        let node = source.find(span)?;

        let parent = node.parent()?;
        if parent.kind() != SyntaxKind::Closure {
            return None;
        }
        let binding = parent.parent()?;
        if binding.kind() != SyntaxKind::LetBinding {
            return None;
        }

        Some(DocComment::parse(&collect_doc_comment(binding)?))
    }

    /// The function a callee node names, if it names one.
    pub(crate) fn func_at(&self, callee: &LinkedNode) -> Option<Func> {
        typst_ide::analyze_expr(self.session().world(), callee)
            .iter()
            .find_map(|(value, _)| match value {
                Value::Func(func) => Some(func.clone()),
                _ => None,
            })
    }

    /// Every named argument a sink-taking function accepts without declaring,
    /// paired with its documentation where there is any.
    fn hidden_named_arguments(
        &self,
        func: &Func,
        docs: &DocComment,
    ) -> Vec<(EcoString, Option<DocEntry>)> {
        let mut keys: Vec<(EcoString, Option<DocEntry>)> = docs
            .style_keys
            .iter()
            // A sink documented among the style keys — cetz's `rect` lists
            // `..style` there — is not a name anyone can pass.
            .filter(|entry| !entry.variadic)
            .map(|entry| (entry.name.clone(), Some(entry.clone())))
            .collect();

        // The root's own dictionary fills in the keys the prose leaves out —
        // `fill` and `stroke` are accepted everywhere and documented nowhere.
        if let Some(root) = &docs.style_root
            && let Some(id) = func.span().id()
        {
            for (name, default) in self.style_dictionary_keys(id, root) {
                if keys.iter().any(|(known, _)| known == &name) {
                    continue;
                }
                keys.push((
                    name.clone(),
                    Some(DocEntry {
                        name,
                        variadic: false,
                        types: None,
                        default: Some(default),
                        docs: EcoString::new(),
                    }),
                ));
            }
        }

        keys
    }

    /// The keys of `<root>` in a style dictionary the defining file imports,
    /// each with its default rendered.
    ///
    /// Looks for the shape cetz and the libraries built on it use: a module
    /// holding a `default` dictionary whose entries are keyed by style root.
    /// Anything else yields nothing.
    fn style_dictionary_keys(&self, file: FileId, root: &str) -> Vec<(EcoString, EcoString)> {
        let Ok(source) = self.session().world().source(file) else {
            return Vec::new();
        };
        let node = LinkedNode::new(source.root());

        let mut imports = Vec::new();
        collect_imports(&node, &mut imports);

        for import in imports {
            let Some(module) = import.cast::<ast::ModuleImport>() else {
                continue;
            };
            let Some(target) = import.find(module.source().span()) else {
                continue;
            };
            let Some(value) = typst_ide::analyze_import(self.session().world(), &target) else {
                continue;
            };
            let Some(scope) = value.scope() else { continue };

            for name in ["default", "default-style"] {
                let Some(binding) = scope.get(name) else {
                    continue;
                };
                let Value::Dict(dict) = binding.read() else {
                    continue;
                };
                let Some(Value::Dict(entry)) = dict.get(root).ok() else {
                    continue;
                };
                return entry
                    .iter()
                    .map(|(key, value)| (key.clone().into(), value.repr()))
                    .collect();
            }
        }

        Vec::new()
    }
}

/// Fill in the documentation upstream could not find for a closure's declared
/// parameters, from the doc comment's own list.
///
/// `typst-ide` looks for a comment written immediately above each parameter,
/// which is not where the ecosystem writes them.
pub(crate) fn enrich_param_items(items: &mut [CompletionItem], docs: &DocComment) {
    for item in items {
        if item.kind != Some(CompletionItemKind::FIELD) || item.documentation.is_some() {
            continue;
        }
        let Some(entry) = docs.param(&item.label) else {
            continue;
        };

        if item.detail.is_none() {
            item.detail = Some(entry.signature().to_string());
        }
        if !entry.docs.is_empty() {
            item.documentation = Some(Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: entry.docs.to_string(),
            }));
        }
    }
}

/// Whether the cursor sits where a named argument may be written, mirroring
/// `typst-ide`'s own rule so the two agree about the replacement range.
fn param_context(source: &Source, cursor: usize, explicit: bool) -> Option<ParamContext<'_>> {
    let root = LinkedNode::new(source.root());
    let leaf = root
        .leaf_at(cursor, Side::Before)
        .or_else(|| root.leaf_at(cursor, Side::After))?;

    // Inside `name: |` the leaf's parent is the `Named` node; the argument
    // list is one further out.
    let parent = leaf.parent()?;
    let parent = match parent.kind() {
        SyntaxKind::Named => parent.parent()?,
        _ => parent,
    };
    let args = parent.get().cast::<ast::Args>()?;

    let grand = parent.parent()?;
    let callee = match grand.get().cast::<ast::Expr>()? {
        ast::Expr::FuncCall(call) => call.callee(),
        ast::Expr::SetRule(set) => set.target(),
        _ => return None,
    };
    let callee = grand.find(callee.span())?;

    // What decides the completion is the nearest paren, comma, or colon
    // before the cursor.
    let mut deciding = leaf.clone();
    while !matches!(
        deciding.kind(),
        SyntaxKind::LeftParen | SyntaxKind::RightParen | SyntaxKind::Comma | SyntaxKind::Colon
    ) {
        let Some(prev) = deciding.prev_leaf() else {
            break;
        };
        deciding = prev;
    }

    // After a colon the cursor is writing a *value*, not a name.
    if !matches!(deciding.kind(), SyntaxKind::LeftParen | SyntaxKind::Comma) {
        return None;
    }
    // `f(1,|` — hugging the comma — only on an explicit request, as upstream.
    if deciding.kind() == SyntaxKind::Comma && deciding.range().end >= cursor && !explicit {
        return None;
    }

    let from = match deciding.next_leaf() {
        Some(next) => cursor.min(next.offset()),
        None => cursor,
    };

    let named = args
        .items()
        .filter_map(|arg| match arg {
            ast::Arg::Named(named) => Some(named.name().get().clone()),
            _ => None,
        })
        .collect();

    Some(ParamContext {
        callee,
        from,
        named,
    })
}

/// Every `#import` in a file, in source order.
fn collect_imports<'a>(node: &LinkedNode<'a>, out: &mut Vec<LinkedNode<'a>>) {
    for child in node.children() {
        if child.kind() == SyntaxKind::ModuleImport {
            out.push(child);
        } else {
            collect_imports(&child, out);
        }
    }
}

/// The `///` comment written above a node.
///
/// The same walk `typst-ide` does internally and does not expose: line
/// comments and block comments directly above the binding, with one leading
/// slash and one leading space taken off each line.
fn collect_doc_comment(node: &LinkedNode) -> Option<EcoString> {
    let mut lines = Vec::new();
    let mut current = node.clone();

    while let Some(prev) = current.prev_sibling_with_trivia() {
        if let Some(comment) = prev.get().cast::<ast::LineComment>() {
            let text = comment.text();
            lines.push(text.strip_prefix('/').unwrap_or(text));
        } else if let Some(comment) = prev.get().cast::<ast::BlockComment>() {
            lines.push(comment.text());
        } else if !matches!(prev.kind(), SyntaxKind::Space | SyntaxKind::Hash) {
            break;
        }
        current = prev;
    }

    if lines.is_empty() {
        return None;
    }

    let mut out = EcoString::new();
    for line in lines.iter().rev() {
        out.push_str(line.strip_prefix(' ').unwrap_or(line));
        out.push('\n');
    }
    Some(out)
}
