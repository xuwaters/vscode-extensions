//! Expression inference — operators (P4-05), members and indexing (P4-04).
//!
//! Every expression node answers a [`Value`]: its type, whether it can be
//! assigned to, and whether it is a constant expression. The three travel
//! together because the rules need all three at once — `a[i] = 1.0` needs the
//! type to check the assignment, the lvalue-ness to allow it at all, and
//! constness only when `a`'s size is being worked out.
//!
//! Every rule here has the same escape hatch: an operand of type
//! [`Type::Unknown`] answers `Unknown` and reports nothing. That is what keeps
//! an extension type, an unfollowed `#include` or a macro this crate could not
//! see from turning into a red squiggle under valid code.

use analyzer_core::spans::ByteSpan;
use glsl_syntax::{NodeId, NodeKind};

use crate::analyzer::{Analyzer, looks_like_extension_name};
use crate::conversions::{common_type, implicitly_convertible};
use crate::diagnostics::SemanticCode;
use crate::symbols::SymbolKind;
use crate::types::{Scalar, Type};
use crate::{Target, builtins};

/// Whether an expression is a constant expression, as `const` initialisers and
/// array sizes need it to be.
///
/// Three-valued on purpose: `No` is the only value a diagnostic may be built
/// on, and everything this crate cannot work out is `Unsure`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Const {
    Yes,
    No,
    Unsure,
}

impl Const {
    /// The constness of an expression built out of two others.
    pub fn and(self, other: Const) -> Const {
        match (self, other) {
            (Const::No, _) | (_, Const::No) => Const::No,
            (Const::Yes, Const::Yes) => Const::Yes,
            _ => Const::Unsure,
        }
    }
}

/// What an expression evaluates to.
#[derive(Debug, Clone)]
pub struct Value {
    pub ty: Type,
    /// Whether it names storage rather than a value.
    pub lvalue: bool,
    /// Why writing to it is an error, when it is one: `"const"`, `"uniform"`,
    /// `"shader input"`.
    pub read_only: Option<&'static str>,
    pub constant: Const,
}

impl Value {
    /// Nothing is known. Deliberately an lvalue: refusing an assignment to
    /// something this crate could not identify would be a false error.
    pub fn unknown() -> Value {
        Value { ty: Type::Unknown, lvalue: true, read_only: None, constant: Const::Unsure }
    }

    pub fn rvalue(ty: Type) -> Value {
        Value { ty, lvalue: false, read_only: None, constant: Const::No }
    }

    pub fn constant(ty: Type) -> Value {
        Value { ty, lvalue: false, read_only: None, constant: Const::Yes }
    }

    pub fn with_constness(mut self, constant: Const) -> Value {
        self.constant = constant;
        self
    }
}

impl Analyzer<'_> {
    /// The expression at `node`, with its type recorded for the editor.
    pub fn expression(&mut self, node: NodeId) -> Value {
        let value = self.deeper(|a| a.expression_inner(node)).unwrap_or_else(Value::unknown);
        self.set_type(node, value.ty.clone());
        value
    }

    fn expression_inner(&mut self, node: NodeId) -> Value {
        match self.tree.kind(node) {
            NodeKind::LiteralExpr => self.literal(node),
            NodeKind::NameExpr => self.name_expr(node),
            NodeKind::ParenExpr => match self.child_expression(node) {
                Some(inner) => self.expression(inner),
                None => Value::unknown(),
            },
            NodeKind::CommaExpr => {
                let children = self.child_expressions(node);
                let mut last = Value::unknown();
                for child in children {
                    last = self.expression(child);
                }
                last
            }
            NodeKind::UnaryExpr => self.unary(node),
            NodeKind::PostfixExpr => self.postfix(node),
            NodeKind::BinaryExpr => self.binary(node),
            NodeKind::AssignExpr => self.assign(node),
            NodeKind::CondExpr => self.conditional(node),
            NodeKind::CallExpr => self.call(node),
            NodeKind::IndexExpr => self.index(node),
            NodeKind::FieldExpr => self.field(node),
            NodeKind::InitializerList => {
                let children = self.child_expressions(node);
                for child in children {
                    self.expression(child);
                }
                Value::unknown()
            }
            // An `Error` node, or anything that is not an expression at all.
            _ => Value::unknown(),
        }
    }

    /// The first child of `node` that is an expression.
    pub fn child_expression(&self, node: NodeId) -> Option<NodeId> {
        self.tree.child_nodes(node).find(|c| self.tree.kind(*c).is_expression())
    }

    /// Every child of `node` that is an expression, in order.
    pub fn child_expressions(&self, node: NodeId) -> Vec<NodeId> {
        self.tree
            .child_nodes(node)
            .filter(|c| {
                self.tree.kind(*c).is_expression()
                    || self.tree.kind(*c) == NodeKind::InitializerList
            })
            .collect()
    }

    /// The operator token spelled directly under a node.
    ///
    /// Read off the token's *kind* rather than its text: the lexer has already
    /// decided which punctuator this is at max munch, and
    /// [`Punct::as_str`](glsl_syntax::Punct::as_str) gives the spelling back as
    /// a `'static` — which is what the rules below match on. Comparing the text
    /// against a table of thirty-five spellings per operator node was one of
    /// the analysis layer's larger costs (RFC 012 P5-10).
    pub fn operator(&self, node: NodeId) -> &'static str {
        for token in self.tree.child_tokens(node) {
            let Some(token) = self.pp.tokens.get(token.index()) else {
                continue;
            };
            if let glsl_syntax::TokenKind::Punct(punct) = token.kind {
                if is_operator(punct) {
                    return punct.as_str();
                }
            }
        }
        ""
    }

    // -- leaves ------------------------------------------------------------

    fn literal(&mut self, node: NodeId) -> Value {
        let text = self.node_text(node);
        // Hex first: `0xf` ends in an `f` that is a digit, not a suffix, and
        // `0x1e5` has an `e` that is not an exponent.
        let hex = text.starts_with("0x") || text.starts_with("0X");
        let float = !hex
            && (text.contains('.')
                || text.contains('e')
                || text.contains('E')
                || text.ends_with('f')
                || text.ends_with('F'));
        let ty = if float {
            if text.ends_with("lf") || text.ends_with("LF") {
                Type::Scalar(Scalar::Double)
            } else {
                Type::FLOAT
            }
        } else if text.ends_with('u') || text.ends_with('U') {
            Type::UINT
        } else if text == "true" || text == "false" {
            Type::BOOL
        } else {
            Type::INT
        };
        Value::constant(ty)
    }

    fn name_expr(&mut self, node: NodeId) -> Value {
        let name = self.node_text(node);
        let span = self.span(node);
        // `true` and `false` are keywords the lexer hands over as words.
        if name == "true" || name == "false" {
            return Value::constant(Type::BOOL);
        }
        if let Some(id) = self.scopes.lookup_one(name) {
            self.record(span, Target::Symbol(id));
            let Some(symbol) = self.symbols.get(id) else {
                return Value::unknown();
            };
            let ty = symbol.ty.clone();
            let kind = symbol.kind;
            let qualifiers = symbol.qualifiers;
            return match kind {
                // A function or a type name used where a value is expected is
                // either a call this walk has not reached yet or a mistake we
                // have nothing useful to say about.
                SymbolKind::Function | SymbolKind::Struct | SymbolKind::Block => {
                    Value::unknown()
                }
                _ => Value {
                    ty,
                    lvalue: true,
                    read_only: qualifiers.read_only(kind),
                    constant: if qualifiers.is_const { Const::Yes } else { Const::No },
                },
            };
        }
        // A type name standing alone: the callee of a constructor, or the base
        // of an array constructor. The `CallExpr` above deals with it.
        if Type::from_name(name).is_some() {
            self.record(span, Target::Type(Type::from_name(name).unwrap_or(Type::Unknown)));
            return Value::unknown();
        }
        if let Some(value) = self.builtin_variable(name, span) {
            return value;
        }
        // A builtin function's name, used without calling it.
        if builtins::lookup_function(name).is_some() {
            return Value::unknown();
        }
        if !looks_like_extension_name(name) && self.names_trusted() {
            self.error(
                SemanticCode::UnknownIdentifier,
                format!("'{name}' is not declared here"),
                span,
            );
        }
        self.record(span, Target::Unresolved);
        Value::unknown()
    }

    /// A predeclared `gl_*` variable, from the generated table or the
    /// hand-written legacy one.
    fn builtin_variable(&mut self, name: &str, span: ByteSpan) -> Option<Value> {
        let found = builtins::lookup_variable(name)?;
        self.record(span, found.target());
        let variable = found.variable();
        if self.availability_enforced()
            && crate::builtins::profile_is_covered(variable.availability(), self.ctx.version)
            && !self.ctx.available(variable.availability(), found.compatibility())
        {
            self.error(
                SemanticCode::NotAvailableInVersion,
                format!(
                    "'{name}' does not exist in GLSL {}{}",
                    self.ctx.version_label(),
                    builtins::replacement_hint(name, self.ctx.version)
                ),
                span,
            );
        } else if self.ctx.stage_known && builtins::is_stage_exclusive(variable) {
            if !variable.stages.contains(self.ctx.stage) {
                let stages: Vec<&str> = variable.stages.stages().map(|s| s.label()).collect();
                self.error(
                    SemanticCode::NotAvailableInStage,
                    format!(
                        "'{name}' only exists in a {} shader, and this is a {} shader",
                        stages.join(" or "),
                        self.ctx.stage.label()
                    ),
                    span,
                );
            }
        }
        let ty = builtins::type_of(variable.ty);
        // The builtin constants of §7.3 are `const int`s; everything else is
        // pipeline state, which no constant expression may read.
        let constant = if name.starts_with("gl_Max") || name.starts_with("gl_Min") {
            Const::Yes
        } else {
            Const::No
        };
        Some(Value { ty, lvalue: true, read_only: None, constant })
    }

    /// Whether the file asked for an extension.
    ///
    /// An `#extension` line can add builtins, types and syntax at any version,
    /// and this crate deliberately models none of them (RFC 012 §2 N3). So a
    /// file that enables one gets no *unknown name* and no *availability*
    /// errors at all: the names it uses may well be ones the extension
    /// declares, and a wrong red squiggle is worse than a missed one.
    pub fn extensions_enabled(&self) -> bool {
        use glsl_syntax::ExtensionBehaviour;
        self.pp.directives.extensions.iter().any(|extension| {
            matches!(
                extension.behaviour,
                ExtensionBehaviour::Enable | ExtensionBehaviour::Require
            )
        })
    }

    /// Whether a name this crate cannot place may be reported at all.
    pub fn names_trusted(&self) -> bool {
        !self.extensions_enabled()
    }

    /// Whether availability diagnostics may fire at all.
    pub fn availability_enforced(&self) -> bool {
        self.ctx.version_known && !self.extensions_enabled()
    }

    // -- operators ---------------------------------------------------------

    fn unary(&mut self, node: NodeId) -> Value {
        let op = self.operator(node);
        let Some(operand) = self.child_expression(node) else {
            return Value::unknown();
        };
        let value = self.expression(operand);
        let span = self.span(node);
        if value.ty.is_unknown() {
            return Value::unknown();
        }
        match op {
            "!" => {
                if value.ty == Type::BOOL {
                    Value::rvalue(Type::BOOL).with_constness(value.constant)
                } else {
                    self.bad_operand(op, &value.ty, span);
                    Value::unknown()
                }
            }
            "~" => {
                if value.ty.is_integral() {
                    Value::rvalue(value.ty.clone()).with_constness(value.constant)
                } else {
                    self.bad_operand(op, &value.ty, span);
                    Value::unknown()
                }
            }
            "+" | "-" => {
                if value.ty.is_numeric() {
                    Value::rvalue(value.ty.clone()).with_constness(value.constant)
                } else {
                    self.bad_operand(op, &value.ty, span);
                    Value::unknown()
                }
            }
            "++" | "--" => {
                self.require_writable(&value, op, span);
                Value::rvalue(value.ty.clone())
            }
            _ => Value::unknown(),
        }
    }

    fn postfix(&mut self, node: NodeId) -> Value {
        let op = self.operator(node);
        let Some(operand) = self.child_expression(node) else {
            return Value::unknown();
        };
        let value = self.expression(operand);
        let span = self.span(node);
        self.require_writable(&value, op, span);
        Value::rvalue(value.ty)
    }

    fn binary(&mut self, node: NodeId) -> Value {
        let op = self.operator(node);
        let operands = self.child_expressions(node);
        let (Some(left), Some(right)) = (operands.first(), operands.get(1)) else {
            for operand in &operands {
                self.expression(*operand);
            }
            return Value::unknown();
        };
        let left = self.expression(*left);
        let right = self.expression(*right);
        let span = self.span(node);
        let constant = left.constant.and(right.constant);
        match self.binary_type(op, &left.ty, &right.ty, span) {
            Some(ty) => Value::rvalue(ty).with_constness(constant),
            None => Value::unknown(),
        }
    }

    /// The type an operator yields, reporting when it has none.
    ///
    /// Returns `None` for "nothing is known", which is also what a reported
    /// error leaves behind — one diagnostic per broken expression, never a
    /// cascade from the parent.
    pub fn binary_type(
        &mut self,
        op: &str,
        left: &Type,
        right: &Type,
        span: ByteSpan,
    ) -> Option<Type> {
        if left.is_unknown() || right.is_unknown() {
            return None;
        }
        match op {
            "&&" | "||" | "^^" => {
                if *left == Type::BOOL && *right == Type::BOOL {
                    Some(Type::BOOL)
                } else {
                    self.bad_operands(op, left, right, span);
                    None
                }
            }
            "==" | "!=" => {
                if left.is_opaque() || right.is_opaque() {
                    self.bad_operands(op, left, right, span);
                    return None;
                }
                if common_type(left, right).is_some() {
                    Some(Type::BOOL)
                } else {
                    self.mismatch(op, left, right, span);
                    None
                }
            }
            "<" | ">" | "<=" | ">=" => {
                if left.is_scalar() && right.is_scalar() && left.is_numeric()
                    && right.is_numeric()
                {
                    Some(Type::BOOL)
                } else {
                    self.bad_operands(op, left, right, span);
                    None
                }
            }
            "<<" | ">>" => {
                if left.is_integral() && right.is_integral() {
                    // The left operand's type is the result's; the right only
                    // has to be an integer.
                    Some(left.clone())
                } else {
                    self.bad_operands(op, left, right, span);
                    None
                }
            }
            "&" | "|" | "^" | "%" => {
                if !left.is_integral() || !right.is_integral() {
                    self.bad_operands(op, left, right, span);
                    return None;
                }
                self.componentwise(op, left, right, span)
            }
            "+" | "-" | "/" => {
                if !left.is_numeric() || !right.is_numeric() {
                    self.bad_operands(op, left, right, span);
                    return None;
                }
                self.componentwise(op, left, right, span)
            }
            "*" => self.multiply(left, right, span),
            _ => None,
        }
    }

    /// The component-wise operators: same shape, or one side a scalar.
    fn componentwise(
        &mut self,
        op: &str,
        left: &Type,
        right: &Type,
        span: ByteSpan,
    ) -> Option<Type> {
        let (Some(lc), Some(rc)) = (left.component(), right.component()) else {
            self.bad_operands(op, left, right, span);
            return None;
        };
        let Some(component) = common_scalar(lc, rc) else {
            self.mismatch(op, left, right, span);
            return None;
        };
        if left.is_scalar() {
            return Some(right.with_component(component));
        }
        if right.is_scalar() {
            return Some(left.with_component(component));
        }
        // Two aggregates have to be the same shape.
        if shape(left) == shape(right) {
            return Some(left.with_component(component));
        }
        self.mismatch(op, left, right, span);
        None
    }

    /// `*` — the one operator with linear algebra in it (§5.10).
    fn multiply(&mut self, left: &Type, right: &Type, span: ByteSpan) -> Option<Type> {
        if !left.is_numeric() || !right.is_numeric() {
            self.bad_operands("*", left, right, span);
            return None;
        }
        let double = matches!(left.component(), Some(Scalar::Double))
            || matches!(right.component(), Some(Scalar::Double));
        match (left, right) {
            // matCxR * vecC = vecR
            (Type::Matrix { cols, rows, .. }, Type::Vector(_, n)) if cols == n => {
                Some(Type::Vector(if double { Scalar::Double } else { Scalar::Float }, *rows))
            }
            // vecR * matCxR = vecC
            (Type::Vector(_, n), Type::Matrix { cols, rows, .. }) if rows == n => {
                Some(Type::Vector(if double { Scalar::Double } else { Scalar::Float }, *cols))
            }
            // matAxB * matCxA = matCxB
            (
                Type::Matrix { cols: c1, rows: r1, .. },
                Type::Matrix { cols: c2, rows: r2, .. },
            ) => {
                if c1 == r2 {
                    Some(Type::Matrix { cols: *c2, rows: *r1, double })
                } else {
                    self.mismatch("*", left, right, span);
                    None
                }
            }
            _ if left.is_matrix() || right.is_matrix() => {
                // A matrix and a scalar scale; a matrix and a vector of the
                // wrong length do not multiply at all.
                if left.is_scalar() || right.is_scalar() {
                    self.componentwise("*", left, right, span)
                } else {
                    self.mismatch("*", left, right, span);
                    None
                }
            }
            _ => self.componentwise("*", left, right, span),
        }
    }

    fn assign(&mut self, node: NodeId) -> Value {
        let op = self.operator(node);
        let operands = self.child_expressions(node);
        let (Some(target), Some(source)) = (operands.first(), operands.get(1)) else {
            for operand in &operands {
                self.expression(*operand);
            }
            return Value::unknown();
        };
        let target = self.expression(*target);
        let source = self.expression(*source);
        let span = self.span(node);
        self.require_writable(&target, "=", span);
        if target.ty.is_unknown() || source.ty.is_unknown() {
            return Value::rvalue(target.ty);
        }
        // A compound assignment is its operator followed by an assignment, so
        // the operator's own rules apply first.
        let source_ty = if op == "=" {
            Some(source.ty.clone())
        } else {
            self.binary_type(op.trim_end_matches('='), &target.ty, &source.ty, span)
        };
        if let Some(source_ty) = source_ty {
            if !implicitly_convertible(&source_ty, &target.ty) {
                let (from, to) =
                    (source_ty.name(&self.structs), target.ty.name(&self.structs));
                self.error(
                    SemanticCode::TypeMismatch,
                    format!("a {from} cannot be assigned to a {to}"),
                    span,
                );
            }
        }
        Value::rvalue(target.ty)
    }

    fn conditional(&mut self, node: NodeId) -> Value {
        let operands = self.child_expressions(node);
        let mut values = Vec::with_capacity(operands.len());
        for operand in &operands {
            values.push(self.expression(*operand));
        }
        let span = self.span(node);
        if let Some(condition) = values.first() {
            if !condition.ty.is_unknown() && condition.ty != Type::BOOL {
                let found = condition.ty.name(&self.structs);
                self.error(
                    SemanticCode::ConditionNotBool,
                    format!("the condition of '?:' must be a bool, and this is a {found}"),
                    self.span(operands[0]),
                );
            }
        }
        let (Some(then), Some(otherwise)) = (values.get(1), values.get(2)) else {
            return Value::unknown();
        };
        if then.ty.is_unknown() || otherwise.ty.is_unknown() {
            return Value::unknown();
        }
        let constant = then.constant.and(otherwise.constant);
        match common_type(&then.ty, &otherwise.ty) {
            Some(ty) => Value::rvalue(ty).with_constness(constant),
            None => {
                let (a, b) = (then.ty.name(&self.structs), otherwise.ty.name(&self.structs));
                self.error(
                    SemanticCode::TypeMismatch,
                    format!("the two branches of '?:' are a {a} and a {b}"),
                    span,
                );
                Value::unknown()
            }
        }
    }

    // -- indexing and members ----------------------------------------------

    fn index(&mut self, node: NodeId) -> Value {
        let operands = self.child_expressions(node);
        let Some(base) = operands.first().copied() else {
            return Value::unknown();
        };
        // `float[3](…)` and `S[2](…)` — a type with a size, not an index.
        if self.type_expression(base).is_some() {
            for operand in operands.iter().skip(1) {
                self.expression(*operand);
            }
            return Value::unknown();
        }
        let base = self.expression(base);
        let index = operands.get(1).map(|i| self.expression(*i));
        let span = self.span(node);
        if base.ty.is_unknown() {
            return Value::unknown();
        }
        let Some(element) = base.ty.indexed() else {
            let found = base.ty.name(&self.structs);
            self.error(
                SemanticCode::NotIndexable,
                format!("a {found} cannot be indexed"),
                span,
            );
            return Value::unknown();
        };
        if let (Some(bound), Some(index_node)) = (base.ty.index_bound(), operands.get(1)) {
            if let Some(value) = crate::consteval::const_int(self, *index_node) {
                if value < 0 || value as u64 >= bound as u64 {
                    let found = base.ty.name(&self.structs);
                    self.error(
                        SemanticCode::IndexOutOfRange,
                        format!("{value} is outside a {found}, which has {bound} of them"),
                        self.span(*index_node),
                    );
                }
            }
        }
        let constant = base.constant.and(index.map_or(Const::Unsure, |i| i.constant));
        Value {
            ty: element,
            lvalue: base.lvalue,
            read_only: base.read_only,
            constant,
        }
    }

    /// The type a name in expression position spells, when it spells one.
    ///
    /// This is the constructor question: `vec4(…)` and `S(…)` are calls whose
    /// callee is a type, and `float[2](…)` is a call whose callee is a type
    /// with a size.
    pub fn type_expression(&mut self, node: NodeId) -> Option<Type> {
        match self.tree.kind(node) {
            NodeKind::NameExpr => {
                let name = self.node_text(node);
                if let Some(ty) = Type::from_name(name) {
                    return Some(ty);
                }
                let id = self.scopes.lookup_one(name)?;
                let symbol = self.symbols.get(id)?;
                matches!(symbol.kind, SymbolKind::Struct).then(|| symbol.ty.clone())
            }
            // `float[2]` and `float[]`.
            NodeKind::IndexExpr => {
                let operands = self.child_expressions(node);
                let base = *operands.first()?;
                let element = self.type_expression(base)?;
                let size = operands
                    .get(1)
                    .and_then(|index| crate::consteval::const_int(self, *index))
                    .filter(|size| *size > 0)
                    .and_then(|size| u32::try_from(size).ok());
                Some(Type::Array(Box::new(element), size))
            }
            _ => None,
        }
    }

    fn field(&mut self, node: NodeId) -> Value {
        let Some(base_node) = self.child_expression(node) else {
            return Value::unknown();
        };
        let base = self.expression(base_node);
        // The member name is the last token under the node; the first is `.`.
        let Some(member_token) = self.tree.child_tokens(node).last() else {
            return Value::unknown();
        };
        let member = self.token_text(member_token);
        let span = self
            .pp
            .tokens
            .get(member_token.index())
            .map_or(self.span(node), |t| t.span);
        if member.is_empty() || member == "." {
            return Value::unknown();
        }
        // `.length` is only ever the method: `a.length()`. The call above reads
        // it, and on its own it is not worth a diagnostic.
        if member == "length" {
            return Value::unknown();
        }
        match &base.ty {
            Type::Unknown => Value::unknown(),
            Type::Struct(id) => {
                let Some(def) = self.structs.get(*id) else {
                    return Value::unknown();
                };
                match def.fields.iter().position(|f| f.name == member) {
                    Some(index) => {
                        let field = &def.fields[index];
                        let ty = field.ty.clone();
                        self.record(span, Target::Field { owner: *id, index });
                        Value {
                            ty,
                            lvalue: base.lvalue,
                            read_only: base.read_only,
                            constant: base.constant,
                        }
                    }
                    None => {
                        let name = def.name.clone();
                        self.error(
                            SemanticCode::UnknownMember,
                            format!("'{name}' has no member called '{member}'"),
                            span,
                        );
                        Value::unknown()
                    }
                }
            }
            Type::Vector(scalar, size) => {
                let (scalar, size) = (*scalar, *size);
                self.record(span, Target::Swizzle);
                self.swizzle(&base, scalar, size, member, span)
            }
            // A one-letter swizzle of a scalar is what some dialects allow and
            // core GLSL does not; saying nothing costs nothing.
            Type::Scalar(_) if member.len() == 1 && is_swizzle_letter(member) => {
                Value { ty: base.ty.clone(), ..base }
            }
            other => {
                let found = other.name(&self.structs);
                self.error(
                    SemanticCode::NotAStruct,
                    format!("a {found} has no member called '{member}'"),
                    span,
                );
                Value::unknown()
            }
        }
    }

    /// `.xyz`, `.rgba`, `.stpq` — §5.5.
    fn swizzle(
        &mut self,
        base: &Value,
        scalar: Scalar,
        size: u8,
        member: &str,
        span: ByteSpan,
    ) -> Value {
        const SETS: &[&str] = &["xyzw", "rgba", "stpq"];
        if member.len() > 4 {
            self.error(
                SemanticCode::BadSwizzle,
                format!("'{member}' selects more than four components"),
                span,
            );
            return Value::unknown();
        }
        let set = SETS.iter().find(|set| member.chars().all(|c| set.contains(c)));
        let Some(set) = set else {
            let mixed = SETS.iter().any(|set| member.chars().any(|c| set.contains(c)));
            self.error(
                SemanticCode::BadSwizzle,
                if mixed {
                    format!(
                        "'{member}' mixes component sets; use one of xyzw, rgba or stpq at a \
                         time"
                    )
                } else {
                    format!("'{member}' is not a component name; use xyzw, rgba or stpq")
                },
                span,
            );
            return Value::unknown();
        };
        let mut indices = Vec::with_capacity(member.len());
        for letter in member.chars() {
            let index = set.find(letter).unwrap_or(0) as u8;
            if index >= size {
                self.error(
                    SemanticCode::BadSwizzle,
                    format!(
                        "'{letter}' is component {} and a {}{size} has {size}",
                        index + 1,
                        scalar.vector_prefix()
                    ),
                    span,
                );
                return Value::unknown();
            }
            indices.push(index);
        }
        let repeated = (1..indices.len()).any(|i| indices[..i].contains(&indices[i]));
        let ty = if indices.len() == 1 {
            Type::Scalar(scalar)
        } else {
            Type::Vector(scalar, indices.len() as u8)
        };
        Value {
            ty,
            // A swizzle that names a component twice is a value, not storage.
            lvalue: base.lvalue && !repeated,
            read_only: base.read_only,
            constant: base.constant,
        }
    }

    // -- shared reporting --------------------------------------------------

    /// Report an assignment, an increment or an `out` argument that cannot be
    /// written to.
    pub fn require_writable(&mut self, value: &Value, op: &str, span: ByteSpan) {
        if value.ty.is_unknown() {
            return;
        }
        if let Some(why) = value.read_only {
            self.error(
                SemanticCode::ReadOnly,
                format!("this is a {why} and '{op}' would write to it"),
                span,
            );
            return;
        }
        if !value.lvalue {
            self.error(
                SemanticCode::NotAnLvalue,
                format!("'{op}' needs something that can be assigned to on its left"),
                span,
            );
        }
    }

    fn bad_operand(&mut self, op: &str, ty: &Type, span: ByteSpan) {
        let found = ty.name(&self.structs);
        self.error(
            SemanticCode::BadOperand,
            format!("'{op}' has no meaning for a {found}"),
            span,
        );
    }

    fn bad_operands(&mut self, op: &str, left: &Type, right: &Type, span: ByteSpan) {
        let (a, b) = (left.name(&self.structs), right.name(&self.structs));
        self.error(
            SemanticCode::BadOperand,
            format!("'{op}' has no meaning for a {a} and a {b}"),
            span,
        );
    }

    fn mismatch(&mut self, op: &str, left: &Type, right: &Type, span: ByteSpan) {
        let (a, b) = (left.name(&self.structs), right.name(&self.structs));
        self.error(
            SemanticCode::TypeMismatch,
            format!("'{op}' cannot combine a {a} and a {b}"),
            span,
        );
    }
}

/// Whether a punctuator is one an expression rule reads as an operator.
///
/// The complement is the punctuation that only ever *structures* an expression
/// — the brackets, the `,` of an argument list, the `;` that ends a statement,
/// the `.` of a member access — and the two preprocessing operators, which
/// cannot survive into a parsed expression at all.
fn is_operator(punct: glsl_syntax::Punct) -> bool {
    use glsl_syntax::Punct;
    !matches!(
        punct,
        Punct::LParen
            | Punct::RParen
            | Punct::LBracket
            | Punct::RBracket
            | Punct::LBrace
            | Punct::RBrace
            | Punct::Dot
            | Punct::Comma
            | Punct::Semi
            | Punct::Hash
            | Punct::HashHash
    )
}

/// The scalar two component types both convert to.
fn common_scalar(left: Scalar, right: Scalar) -> Option<Scalar> {
    if left == right {
        return Some(left);
    }
    let a = Type::Scalar(left);
    let b = Type::Scalar(right);
    common_type(&a, &b).and_then(|ty| ty.component())
}

/// A type's shape, ignoring its component type — what "the same shape" means
/// for a component-wise operator.
fn shape(ty: &Type) -> (u8, u8) {
    match ty {
        Type::Scalar(_) => (1, 1),
        Type::Vector(_, n) => (1, *n),
        Type::Matrix { cols, rows, .. } => (*cols, *rows),
        _ => (0, 0),
    }
}

fn is_swizzle_letter(member: &str) -> bool {
    matches!(member, "x" | "y" | "z" | "w" | "r" | "g" | "b" | "a" | "s" | "t" | "p" | "q")
}
