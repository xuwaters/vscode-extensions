//! Statements: scopes inside a body, and the rules of §6 (P4-08).
//!
//! Five rules live here, and each is one a compiler would report and an editor
//! should: a `return` that does not match its function, a `discard` outside a
//! fragment shader, a `break` or `continue` with nothing to leave, a condition
//! that is not a `bool`, and a `const` without a constant value. Two more are
//! warnings rather than errors — code after a `return`, and a non-`void`
//! function with no `return` in it at all — because neither stops the shader
//! from compiling and both are usually a file mid-edit.
//!
//! Scopes follow §4.2 exactly: a compound statement opens one, a `for` header
//! opens one that covers the loop body, and a condition that declares a name
//! (`while (bool ok = next())`) puts it in the statement's own scope.

use glsl_syntax::{NodeId, NodeKind};

use crate::analyzer::Analyzer;
use crate::conversions::implicitly_convertible;
use crate::diagnostics::SemanticCode;
use crate::symbols::{Qualifiers, Symbol, SymbolKind};
use crate::types::Type;

impl Analyzer<'_> {
    /// A `{ … }`. Answers whether it contains a `return` anywhere, which is all
    /// [`Analyzer::function_body`]'s missing-return warning needs to know.
    pub fn compound(&mut self, node: NodeId) -> bool {
        self.scopes.push();
        let statements: Vec<NodeId> = self.tree.child_nodes(node).collect();
        let mut unreachable_reported = false;
        let mut terminated = false;
        for statement in statements {
            if terminated && !unreachable_reported {
                unreachable_reported = true;
                let span = self.span(statement);
                self.warn(
                    SemanticCode::UnreachableCode,
                    "this can never run: the statement before it always leaves",
                    span,
                );
            }
            terminated |= matches!(
                self.tree.kind(statement),
                NodeKind::ReturnStmt
                    | NodeKind::BreakStmt
                    | NodeKind::ContinueStmt
                    | NodeKind::DiscardStmt
            );
            self.statement(statement);
        }
        self.scopes.pop();
        self.tree
            .descendants(node)
            .any(|d| self.tree.kind(d) == NodeKind::ReturnStmt)
    }

    /// One statement of any kind.
    pub fn statement(&mut self, node: NodeId) {
        if self.deeper(|a| a.statement_inner(node)).is_none() {
            // Deeper than this walk goes. The tree below is left untyped, which
            // is exactly what an editor should do with a pathological file.
        }
    }

    fn statement_inner(&mut self, node: NodeId) {
        match self.tree.kind(node) {
            NodeKind::CompoundStmt => {
                self.compound(node);
            }
            NodeKind::DeclStmt => self.declaration_statement(node),
            NodeKind::ExprStmt => {
                if let Some(expr) = self.child_expression(node) {
                    self.expression(expr);
                }
            }
            NodeKind::IfStmt => self.branch(node, true),
            NodeKind::SwitchStmt => {
                self.switch_depth += 1;
                self.branch(node, false);
                self.switch_depth -= 1;
            }
            NodeKind::WhileStmt | NodeKind::DoWhileStmt => {
                self.loop_depth += 1;
                self.branch(node, true);
                self.loop_depth -= 1;
            }
            NodeKind::ForStmt => self.for_statement(node),
            NodeKind::CaseLabel => {
                if let Some(expr) = self.child_expression(node) {
                    self.expression(expr);
                }
            }
            NodeKind::ReturnStmt => self.return_statement(node),
            NodeKind::BreakStmt => {
                if self.loop_depth == 0 && self.switch_depth == 0 {
                    let span = self.span(node);
                    self.error(
                        SemanticCode::MisplacedJump,
                        "'break' needs a loop or a switch to leave",
                        span,
                    );
                }
            }
            NodeKind::ContinueStmt => {
                if self.loop_depth == 0 {
                    let span = self.span(node);
                    self.error(
                        SemanticCode::MisplacedJump,
                        "'continue' needs a loop to continue",
                        span,
                    );
                }
            }
            NodeKind::DiscardStmt => {
                if self.ctx.stage_known && self.ctx.stage != glsl_spec::Stage::Fragment {
                    let span = self.span(node);
                    let stage = self.ctx.stage.label();
                    self.error(
                        SemanticCode::DiscardOutsideFragment,
                        format!("'discard' only exists in a fragment shader, and this is a \
                                 {stage} shader"),
                        span,
                    );
                }
            }
            // A statement kind with nothing of its own to check — an
            // `EmptyStmt`, a `PrecisionDecl`, an `Error` — still has children
            // worth resolving.
            _ => {
                let children: Vec<NodeId> = self.tree.child_nodes(node).collect();
                for child in children {
                    if self.tree.kind(child).is_expression() {
                        self.expression(child);
                    } else {
                        self.statement(child);
                    }
                }
            }
        }
    }

    /// The shared shape of `if`, `while`, `do`, `switch`: a condition and one
    /// or two statements. `boolean` says whether the condition must be a bool —
    /// a `switch` selector is an integer.
    fn branch(&mut self, node: NodeId, boolean: bool) {
        // The condition may declare, and what it declares is visible in the
        // body, so the whole statement gets a scope.
        self.scopes.push();
        let children: Vec<NodeId> = self.tree.child_nodes(node).collect();
        for child in children {
            match self.tree.kind(child) {
                NodeKind::Condition => self.condition(child, boolean),
                kind if kind.is_expression() => {
                    self.expression(child);
                }
                _ => self.statement(child),
            }
        }
        self.scopes.pop();
    }

    fn for_statement(&mut self, node: NodeId) {
        self.scopes.push();
        self.loop_depth += 1;
        let children: Vec<NodeId> = self.tree.child_nodes(node).collect();
        for child in children {
            match self.tree.kind(child) {
                NodeKind::Condition => self.condition(child, true),
                kind if kind.is_expression() => {
                    self.expression(child);
                }
                _ => self.statement(child),
            }
        }
        self.loop_depth -= 1;
        self.scopes.pop();
    }

    /// An `if`/`while`/`for` header, which §9 lets declare a name.
    fn condition(&mut self, node: NodeId, boolean: bool) {
        let ty = match self.tree.child_of_kind(node, NodeKind::TypeSpec) {
            Some(spec) => {
                let base = self.type_of_spec(spec);
                let qualifiers = self.qualifiers_of(node);
                match self.tree.child_of_kind(node, NodeKind::Declarator) {
                    Some(declarator) => self.local_declarator(declarator, &base, qualifiers),
                    None => base,
                }
            }
            None => match self.child_expression(node) {
                Some(expr) => self.expression(expr).ty,
                None => Type::Unknown,
            },
        };
        if boolean && !ty.is_unknown() && ty != Type::BOOL {
            let span = self.span(node);
            let found = ty.name(&self.structs);
            self.error(
                SemanticCode::ConditionNotBool,
                format!("a condition must be a bool, and this is a {found}"),
                span,
            );
        }
    }

    /// `float x = 1.0, y[2];` inside a body.
    fn declaration_statement(&mut self, node: NodeId) {
        let declarations: Vec<NodeId> = self
            .tree
            .child_nodes(node)
            .filter(|c| self.tree.kind(*c) == NodeKind::Declaration)
            .collect();
        for declaration in declarations {
            self.local_declaration(declaration);
        }
    }

    fn local_declaration(&mut self, node: NodeId) {
        let qualifiers = self.qualifiers_of(node);
        let Some(spec) = self.tree.child_of_kind(node, NodeKind::TypeSpec) else {
            return;
        };
        let base = self.type_of_spec(spec);
        let declarators: Vec<NodeId> = self
            .tree
            .child_nodes(node)
            .filter(|c| self.tree.kind(*c) == NodeKind::Declarator)
            .collect();
        for declarator in declarators {
            self.local_declarator(declarator, &base, qualifiers);
        }
        self.check_initialisers(node, false);
    }

    /// One local name, declared and answered with its type.
    fn local_declarator(
        &mut self,
        declarator: NodeId,
        base: &Type,
        qualifiers: Qualifiers,
    ) -> Type {
        let Some((name, name_span)) = self.name_of(declarator) else {
            return base.clone();
        };
        let ty = self.declarator_type(declarator, base);
        let const_value = if qualifiers.is_const {
            self.tree
                .child_of_kind(declarator, NodeKind::Initializer)
                .and_then(|init| self.child_expression(init))
                .and_then(|expr| crate::consteval::const_int(self, expr))
        } else {
            None
        };
        self.declare(Symbol {
            name: name.to_string(),
            kind: SymbolKind::Local,
            ty: ty.clone(),
            qualifiers,
            name_span,
            full_span: self.span(declarator),
            signature: None,
            struct_id: None,
            is_prototype: false,
            const_value,
        });
        ty
    }

    fn return_statement(&mut self, node: NodeId) {
        let expr = self.child_expression(node);
        let span = self.span(node);
        let expected = self.return_type.clone();
        match (expr, &expected) {
            (Some(expr), Type::Void) => {
                self.expression(expr);
                self.error(
                    SemanticCode::ReturnMismatch,
                    "this function is void, so its 'return' takes no value",
                    span,
                );
            }
            (Some(expr), expected) => {
                let value = self.expression(expr);
                if !expected.is_unknown()
                    && !value.ty.is_unknown()
                    && !implicitly_convertible(&value.ty, expected)
                {
                    let (from, to) =
                        (value.ty.name(&self.structs), expected.name(&self.structs));
                    self.error(
                        SemanticCode::ReturnMismatch,
                        format!("this function returns {to}, and this is a {from}"),
                        self.span(expr),
                    );
                }
            }
            (None, Type::Void | Type::Unknown) => {}
            (None, expected) => {
                let expected = expected.name(&self.structs);
                self.error(
                    SemanticCode::ReturnMismatch,
                    format!("this function returns {expected}, so 'return' needs a value"),
                    span,
                );
            }
        }
    }
}
