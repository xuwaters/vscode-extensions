//! Inlay hints: parameter names at call sites.
//!
//! Derived from the callee's `Func` metadata, which is all upstream exposes —
//! and all that is needed, since the hint names the *declared* parameter rather
//! than any evaluated value.
//!
//! Off by default: in markup-heavy files these add noise faster than
//! information.

use lsp_types::{InlayHint, InlayHintKind, InlayHintLabel, InlayHintParams};
use typst::foundations::{Func, Value};
use typst::syntax::ast::AstNode;
use typst::syntax::{LinkedNode, Source, SyntaxKind, ast};

use crate::convert::{offset_to_position, range_from_lsp};
use crate::{Ports, Server};

impl<Q: Ports> Server<Q> {
    /// `textDocument/inlayHint`.
    pub fn inlay_hints(&mut self, params: InlayHintParams) -> Option<Vec<InlayHint>> {
        if !self.settings().inlay_hints.enabled {
            return None;
        }

        let (_, source) = self.source_of(&params.text_document.uri)?;
        let byte_range = range_from_lsp(&source, params.range);

        let mut out = Vec::new();
        self.collect_hints(&LinkedNode::new(source.root()), &source, &byte_range, &mut out);
        Some(out)
    }

    fn collect_hints(
        &self,
        node: &LinkedNode,
        source: &Source,
        window: &std::ops::Range<usize>,
        out: &mut Vec<InlayHint>,
    ) {
        // Skip subtrees the client cannot see.
        let range = node.range();
        if range.end < window.start || range.start > window.end {
            return;
        }

        if node.kind() == SyntaxKind::FuncCall
            && let Some(call) = node.cast::<ast::FuncCall>()
        {
            self.hints_for_call(node, &call, source, out);
        }

        for child in node.children() {
            self.collect_hints(&child, source, window, out);
        }
    }

    fn hints_for_call(
        &self,
        node: &LinkedNode,
        call: &ast::FuncCall,
        source: &Source,
        out: &mut Vec<InlayHint>,
    ) {
        let Some(func) = self.resolve_callee(node, call) else { return };

        // Positional parameters, in declaration order.
        let positional: Vec<String> = func
            .params()
            .filter(|param| param.positional() && !param.variadic())
            .filter_map(|param| param.name().map(str::to_string))
            .collect();

        let Some(args) = node.children().find(|child| child.kind() == SyntaxKind::Args)
        else {
            return;
        };

        let mut index = 0;
        for arg in args.children() {
            match arg.kind() {
                // Named arguments already say what they are.
                SyntaxKind::Named | SyntaxKind::Spread => {
                    index += 1;
                    continue;
                }
                kind if kind.is_trivia() || is_punctuation(kind) => continue,
                _ => {}
            }

            let Some(name) = positional.get(index) else { break };
            index += 1;

            // An argument that already reads as its parameter name adds nothing.
            if source.text().get(arg.range()).is_some_and(|text| text == name) {
                continue;
            }

            out.push(InlayHint {
                position: offset_to_position(source, arg.range().start),
                label: InlayHintLabel::String(format!("{name}:")),
                kind: Some(InlayHintKind::PARAMETER),
                text_edits: None,
                tooltip: None,
                padding_left: Some(false),
                padding_right: Some(true),
                data: None,
            });
        }
    }

    /// The function a call refers to, when it can be resolved statically.
    pub(crate) fn resolve_callee(&self, node: &LinkedNode, call: &ast::FuncCall) -> Option<Func> {
        let callee = node.find(call.callee().span())?;
        let values = typst_ide::analyze_expr(self.session().world(), &callee);
        values.iter().find_map(|(value, _)| match value {
            Value::Func(func) => Some(func.clone()),
            _ => None,
        })
    }
}

fn is_punctuation(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::LeftParen
            | SyntaxKind::RightParen
            | SyntaxKind::Comma
            | SyntaxKind::LeftBracket
            | SyntaxKind::RightBracket
    )
}
