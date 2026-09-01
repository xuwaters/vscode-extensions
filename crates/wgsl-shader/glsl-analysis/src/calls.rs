//! Call resolution: constructors, user functions and builtins (P4-06).
//!
//! The parser deliberately cannot tell these apart — `vec4(…)`, `f(…)` and
//! `float[2](…)` are all one node kind — so this is where the symbol table
//! settles it. In order:
//!
//! 1. `a.length()`, which is a method and nothing else.
//! 2. A callee that names a **type**: a constructor, checked by §5.4.
//! 3. A callee that names **user functions**: overload resolution over the
//!    signatures in scope.
//! 4. A callee that names a **builtin**: the same resolution over the spec
//!    tables, with the generic families expanded as it goes.
//!
//! ## Ranking (§6.1)
//!
//! Each argument scores [`conversion_cost`]; a candidate scores the sum. The
//! lowest total wins. Two candidates tied at a *non-zero* total, with different
//! return types, are genuinely ambiguous and say so; tied at zero they are the
//! same call written twice and the first is taken, which is what keeps
//! `max(1.0, 2.0)` — matching both `max(genType, genType)` and
//! `max(genType, float)` exactly — from being reported as a puzzle.
//!
//! ## Generic families
//!
//! `glsl-spec` keeps families symbolic (`genType`, `gsampler2D`, `gvec4`), so a
//! candidate binds each family once and every later occurrence must agree.
//! Resolving one family from another — the `gvec4` a `gsampler2D` argument
//! implies — goes by *shape* first (a `vec3` argument makes `genBType` mean
//! `bvec3`) and by *class* second (an `isampler2D` makes `gvec4` mean `ivec4`).

use analyzer_core::spans::ByteSpan;
use glsl_spec::{BuiltinFunction, FamilyId, Overload, TypeRef, Version};
use glsl_syntax::{NodeId, NodeKind};

use crate::analyzer::{Analyzer, looks_like_extension_name};
use crate::conversions::{Constructed, conversion_cost, construct, implicitly_convertible};
use crate::diagnostics::SemanticCode;
use crate::expr::{Const, Value};
use crate::symbols::{SymbolId, SymbolKind};
use crate::types::Type;
use crate::{Target, builtins};

/// One argument, kept with its node so a diagnostic can point at it.
struct Argument {
    node: NodeId,
    value: Value,
}

/// The families one candidate bound, as `(family, the type it means)`.
///
/// Keyed by [`FamilyId`] rather than by the family's name: a candidate is
/// scored for every overload of every builtin call, and a `String` per binding
/// was a measurable share of the analysis layer's allocation (RFC 012 P5-10).
type Bindings = Vec<(FamilyId, Type)>;

impl Analyzer<'_> {
    /// A `CallExpr`: a call, a constructor, or an array constructor.
    pub fn call(&mut self, node: NodeId) -> Value {
        let children: Vec<NodeId> = self.tree.child_nodes(node).collect();
        let callee = children
            .iter()
            .copied()
            .find(|c| self.tree.kind(*c) != NodeKind::ArgumentList);
        let list = children
            .iter()
            .copied()
            .find(|c| self.tree.kind(*c) == NodeKind::ArgumentList);
        let Some(callee) = callee else {
            return Value::unknown();
        };

        // `a.length()` — the one method GLSL has.
        if self.tree.kind(callee) == NodeKind::FieldExpr {
            return self.length_method(callee, list);
        }

        let arguments = self.arguments(list);
        let span = self.span(node);

        // A constructor: the callee names a type.
        if let Some(target) = self.type_expression(callee) {
            return self.constructor(target, &arguments, span);
        }

        let (name, name_span) = match self.tree.kind(callee) {
            NodeKind::NameExpr => (self.node_text(callee), self.span(callee)),
            // Anything else — `(f)(x)`, `table[i](x)` — is not a call GLSL has.
            _ => {
                self.expression(callee);
                return Value::unknown();
            }
        };

        let candidates: Vec<SymbolId> = self
            .scopes
            .lookup(name)
            .iter()
            .copied()
            .filter(|id| {
                self.symbols.get(*id).is_some_and(|s| s.kind == SymbolKind::Function)
            })
            .collect();
        if !candidates.is_empty() {
            return self.user_call(name, name_span, &candidates, &arguments, span);
        }
        // A name that is in scope but is not a function at all.
        if let Some(id) = self.scopes.lookup_one(name) {
            self.record(name_span, Target::Symbol(id));
            if let Some(symbol) = self.symbols.get(id) {
                let kind = match symbol.kind {
                    SymbolKind::Block => "an interface block",
                    SymbolKind::Parameter => "a parameter",
                    _ => "a variable",
                };
                self.error(
                    SemanticCode::UnknownFunction,
                    format!("'{name}' is {kind}, not a function"),
                    name_span,
                );
            }
            return Value::unknown();
        }
        if let Some(found) = builtins::lookup_function(name) {
            return self.builtin_call(name, name_span, found, &arguments, span);
        }
        if !looks_like_extension_name(name) && self.names_trusted() {
            self.error(
                SemanticCode::UnknownFunction,
                format!("'{name}' is not a function this shader declares"),
                name_span,
            );
        }
        self.record(name_span, Target::Unresolved);
        Value::unknown()
    }

    /// Evaluate the argument list. `f(void)` passes nothing.
    fn arguments(&mut self, list: Option<NodeId>) -> Vec<Argument> {
        let Some(list) = list else {
            return Vec::new();
        };
        let nodes = self.child_expressions(list);
        if nodes.len() == 1
            && self.tree.kind(nodes[0]) == NodeKind::NameExpr
            && self.node_text(nodes[0]) == "void"
        {
            return Vec::new();
        }
        nodes
            .into_iter()
            .map(|node| {
                let value = self.expression(node);
                Argument { node, value }
            })
            .collect()
    }

    /// `array.length()`, and its vector and matrix forms (4.20+).
    fn length_method(&mut self, callee: NodeId, list: Option<NodeId>) -> Value {
        let member = self
            .tree
            .child_tokens(callee)
            .last()
            .map(|t| self.token_text(t))
            .unwrap_or("");
        let base = match self.child_expression(callee) {
            Some(base) => self.expression(base),
            None => Value::unknown(),
        };
        for argument in self.arguments(list) {
            let _ = argument;
        }
        if member != "length" {
            // `a.foo()` is not a call GLSL has; the field access has already
            // said whatever there was to say about `foo`.
            return Value::unknown();
        }
        match base.ty {
            Type::Array(_, size) => {
                Value::rvalue(Type::INT).with_constness(if size.is_some() {
                    Const::Yes
                } else {
                    Const::No
                })
            }
            Type::Vector(..) | Type::Matrix { .. } => Value::constant(Type::INT),
            _ => Value::unknown(),
        }
    }

    /// A constructor call — §5.4, by way of [`construct`].
    fn constructor(&mut self, target: Type, arguments: &[Argument], span: ByteSpan) -> Value {
        let types: Vec<Type> = arguments.iter().map(|a| a.value.ty.clone()).collect();
        // An array constructor with no size takes it from the arguments.
        let target = match target {
            Type::Array(element, None) if !arguments.is_empty() => {
                Type::Array(element, Some(arguments.len() as u32))
            }
            other => other,
        };
        match construct(&target, &types, &self.structs) {
            Constructed::Ok | Constructed::Unsure => {}
            Constructed::Rejected(why) => {
                self.error(SemanticCode::BadConstructor, why, span);
            }
        }
        let constant = arguments
            .iter()
            .fold(Const::Yes, |accumulated, a| accumulated.and(a.value.constant));
        Value::rvalue(target).with_constness(constant)
    }

    /// A call to a function the file declares.
    fn user_call(
        &mut self,
        name: &str,
        name_span: ByteSpan,
        candidates: &[SymbolId],
        arguments: &[Argument],
        span: ByteSpan,
    ) -> Value {
        let mut best: Option<(u32, SymbolId)> = None;
        let mut ties = 0usize;
        let mut tie_return: Option<Type> = None;
        for id in candidates {
            let Some(symbol) = self.symbols.get(*id) else {
                continue;
            };
            let Some(signature) = symbol.signature.clone() else {
                continue;
            };
            if signature.params.len() != arguments.len() {
                continue;
            }
            let mut cost = 0u32;
            let mut fits = true;
            for (param, argument) in signature.params.iter().zip(arguments) {
                match conversion_cost(&argument.value.ty, &param.ty) {
                    Some(step) => cost += step,
                    None => {
                        fits = false;
                        break;
                    }
                }
            }
            if !fits {
                continue;
            }
            match best {
                Some((previous, _)) if previous < cost => {}
                Some((previous, _)) if previous == cost => {
                    ties += 1;
                    if tie_return.as_ref() != Some(&signature.ret) {
                        tie_return = Some(signature.ret.clone());
                    }
                }
                _ => {
                    best = Some((cost, *id));
                    ties = 0;
                    tie_return = Some(signature.ret.clone());
                }
            }
        }

        if let Some((cost, id)) = best {
            self.record(name_span, Target::Symbol(id));
            // Two candidates that fit equally well, and disagree about what
            // they answer, is the ambiguity §6.1 makes an error.
            if ties > 0 && cost > 0 {
                let best_ret =
                    self.symbols.get(id).and_then(|s| s.signature.as_ref()).map(|s| &s.ret);
                if tie_return.as_ref() != best_ret {
                    self.error(
                        SemanticCode::AmbiguousCall,
                        format!(
                            "several declarations of '{name}' match these arguments equally \
                             well"
                        ),
                        span,
                    );
                }
            }
            let signature = self.symbols.get(id).and_then(|s| s.signature.clone());
            if let Some(signature) = signature {
                for (param, argument) in signature.params.iter().zip(arguments) {
                    if param.writes {
                        let value = argument.value.clone();
                        let argument_span = self.span(argument.node);
                        self.require_writable(&value, "out", argument_span);
                    }
                }
                return Value::rvalue(signature.ret);
            }
            return Value::unknown();
        }

        // Nothing fits. With one declaration the message can be specific.
        if candidates.len() == 1 {
            let symbol = self.symbols.get(candidates[0]).cloned();
            if let Some(symbol) = symbol {
                self.record(name_span, Target::Symbol(candidates[0]));
                if let Some(signature) = &symbol.signature {
                    if signature.params.len() != arguments.len() {
                        self.error(
                            SemanticCode::ArgumentCount,
                            format!(
                                "'{name}' takes {} argument{}, and this passes {}",
                                signature.params.len(),
                                if signature.params.len() == 1 { "" } else { "s" },
                                arguments.len()
                            ),
                            span,
                        );
                        return Value::rvalue(signature.ret.clone());
                    }
                    for (param, argument) in signature.params.iter().zip(arguments) {
                        if argument.value.ty.is_unknown() || param.ty.is_unknown() {
                            continue;
                        }
                        if !implicitly_convertible(&argument.value.ty, &param.ty) {
                            let (from, to) = (
                                argument.value.ty.name(&self.structs),
                                param.ty.name(&self.structs),
                            );
                            let argument_span = self.span(argument.node);
                            self.error(
                                SemanticCode::ArgumentType,
                                format!(
                                    "'{}' is a {to} and this argument is a {from}",
                                    if param.name.is_empty() {
                                        "this parameter".to_string()
                                    } else {
                                        param.name.clone()
                                    }
                                ),
                                argument_span,
                            );
                        }
                    }
                    return Value::rvalue(signature.ret.clone());
                }
            }
            return Value::unknown();
        }
        self.record(name_span, Target::Unresolved);
        let types = self.argument_list(arguments);
        self.error(
            SemanticCode::NoMatchingOverload,
            format!("no declaration of '{name}' takes ({types})"),
            span,
        );
        Value::unknown()
    }

    /// A call to a builtin, from either spec table.
    fn builtin_call(
        &mut self,
        name: &str,
        name_span: ByteSpan,
        found: builtins::FoundFunction,
        arguments: &[Argument],
        span: ByteSpan,
    ) -> Value {
        self.record(name_span, found.target());
        let function = found.function();
        if self.availability_enforced()
            && builtins::profile_is_covered(function.availability(), self.ctx.version)
            && !self.ctx.available(function.availability(), found.compatibility())
        {
            self.error(
                SemanticCode::NotAvailableInVersion,
                format!(
                    "'{name}' does not exist in GLSL {}{}",
                    self.ctx.version_label(),
                    builtins::replacement_hint(name, self.ctx.version)
                ),
                name_span,
            );
            return Value::unknown();
        }

        // The per-overload masks are best-effort (research/docs-gl.md §5.1), so
        // a call that matches nothing in this version is retried against every
        // overload before anything is reported.
        let version = self.ctx.version_known.then_some(self.ctx.version);
        if let Some(value) = self.match_overloads(function, version, arguments) {
            return value;
        }
        if version.is_some() {
            if let Some(value) = self.match_overloads(function, None, arguments) {
                return value;
            }
        }
        // Two ways to fail, and only one of them is trustworthy. A call whose
        // *arity* matches no overload is plainly wrong. A call whose arity
        // matches but whose types do not may equally be a hole in the tables:
        // docs.gl's sampler and image pages do not list every sampler type
        // each function takes. So when an argument is opaque and the arity was
        // right, nothing is said.
        let arity_fits = function.overloads.iter().any(|overload| {
            let (minimum, maximum) = overload.arity();
            arguments.len() >= minimum && arguments.len() <= maximum
        });
        let opaque = arguments.iter().any(|a| a.value.ty.is_opaque());
        // …and an `#extension` line can add overloads to any builtin, which is
        // the third way this can be our gap rather than the shader's.
        if (arity_fits && opaque) || !self.names_trusted() {
            return Value::unknown();
        }
        let types = self.argument_list(arguments);
        self.error(
            SemanticCode::NoMatchingOverload,
            format!("no overload of '{name}' takes ({types})"),
            span,
        );
        Value::unknown()
    }

    /// The best overload for these arguments, and the value it yields.
    ///
    /// `version` restricts the candidates to the overloads that version has;
    /// `None` considers every overload, which is the retry the caller makes
    /// when the masks turn out to be the incomplete half.
    ///
    /// The two binding buffers are reused across candidates and swapped when a
    /// new best appears, so scoring a builtin with a dozen overloads costs at
    /// most the two allocations, not two per candidate.
    fn match_overloads(
        &mut self,
        function: &'static BuiltinFunction,
        version: Option<Version>,
        arguments: &[Argument],
    ) -> Option<Value> {
        let mut best: Option<(u32, &'static Overload)> = None;
        let mut bindings: Bindings = Vec::new();
        let mut scratch: Bindings = Vec::new();
        for overload in function.overloads {
            if version.is_some_and(|v| !overload.availability().contains(v)) {
                continue;
            }
            let (minimum, maximum) = overload.arity();
            if arguments.len() < minimum || arguments.len() > maximum {
                continue;
            }
            scratch.clear();
            let Some(cost) = self.score_overload(overload, arguments, &mut scratch) else {
                continue;
            };
            match &best {
                Some((previous, _)) if *previous <= cost => {}
                _ => {
                    best = Some((cost, overload));
                    std::mem::swap(&mut bindings, &mut scratch);
                }
            }
        }
        let (_, overload) = best?;
        for (param, argument) in overload.params.iter().zip(arguments) {
            if param.flow.writes() {
                let value = argument.value.clone();
                let argument_span = self.span(argument.node);
                self.require_writable(&value, param.flow.keyword(), argument_span);
            }
        }
        let ret = resolve_type_ref(overload.ret, &bindings);
        // §4.3.3 lets a constant expression call a builtin — all but the
        // texture lookups, which this crate does not separate out. `Unsure`
        // rather than `No`, so `const float c = sin(1.0);` is left alone.
        Some(Value::rvalue(ret).with_constness(Const::Unsure))
    }

    /// What one overload costs for these arguments. The families it binds are
    /// appended to `bindings`, which the caller supplies empty.
    fn score_overload(
        &self,
        overload: &'static Overload,
        arguments: &[Argument],
        bindings: &mut Bindings,
    ) -> Option<u32> {
        let mut cost = 0u32;
        for (param, argument) in overload.params.iter().zip(arguments) {
            let argument_ty = &argument.value.ty;
            match param.ty {
                TypeRef::Concrete(name) => {
                    let Some(param_ty) = Type::from_name(name) else {
                        // A parameter type no table here models — an extension
                        // type in a hand-written entry. Accept it and move on.
                        cost += 8;
                        continue;
                    };
                    cost += conversion_cost(argument_ty, &param_ty)?;
                }
                TypeRef::Family(id) => {
                    if let Some((_, bound)) = bindings.iter().find(|(bound, _)| *bound == id) {
                        let bound = bound.clone();
                        cost += conversion_cost(argument_ty, &bound)?;
                        continue;
                    }
                    // An argument this crate could not type must not *decide*
                    // what the family means: binding `genType` to its first
                    // member because the argument was unknown would make
                    // `lessThan(i64vec3(0), i64vec3(1))` answer `bvec2`, and
                    // then the assignment around it would be reported as a
                    // mismatch that only the guess created.
                    if argument_ty.is_unknown() {
                        cost += 64;
                        continue;
                    }
                    // Bind the family to whichever member fits best.
                    let mut chosen: Option<(u32, &Type)> = None;
                    for member_ty in builtins::family_members(id).iter().flatten() {
                        let Some(step) = conversion_cost(argument_ty, member_ty) else {
                            continue;
                        };
                        if chosen.as_ref().is_none_or(|(best, _)| step < *best) {
                            chosen = Some((step, member_ty));
                        }
                    }
                    let (step, member_ty) = chosen?;
                    cost += step;
                    bindings.push((id, member_ty.clone()));
                }
            }
        }
        Some(cost)
    }

    /// `(vec3, float)` — the argument list a "nothing matches" message prints.
    fn argument_list(&self, arguments: &[Argument]) -> String {
        arguments
            .iter()
            .map(|a| a.value.ty.name(&self.structs))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// A prototype's type, with the families this call bound.
fn resolve_type_ref(reference: TypeRef, bindings: &[(FamilyId, Type)]) -> Type {
    match reference {
        TypeRef::Concrete(name) => Type::from_name(name).unwrap_or(Type::Unknown),
        TypeRef::Family(id) => {
            let family = id.get();
            if let Some((_, bound)) = bindings.iter().find(|(bound, _)| *bound == id) {
                return bound.clone();
            }
            // A family that no argument bound — the `gvec4` a `gsampler2D`
            // implies. Shape first, then class.
            for (_, bound) in bindings {
                if let Some(found) = member_of_shape(id, bound) {
                    return found;
                }
            }
            for (_, bound) in bindings {
                if let Some(found) = member_of_class(id, family, bound) {
                    return found;
                }
            }
            Type::Unknown
        }
    }
}

/// The family member with as many components as `template` — `vec3` makes
/// `genBType` mean `bvec3`.
fn member_of_shape(id: FamilyId, template: &Type) -> Option<Type> {
    let wanted = template.component_count()?;
    builtins::family_members(id)
        .iter()
        .flatten()
        .find(|member| member.component_count() == Some(wanted))
        .cloned()
}

/// The family member of the same class — `isampler2D` makes `gvec4` mean
/// `ivec4`.
fn member_of_class(
    id: FamilyId,
    family: &'static glsl_spec::Family,
    template: &Type,
) -> Option<Type> {
    let wanted = class_of(template);
    let at = family.members.iter().position(|member| class_of_name(member) == wanted)?;
    builtins::family_members(id).get(at)?.clone()
}

/// 0 for the plain family member, 1 for the `i`-prefixed one, 2 for the `u`.
fn class_of(ty: &Type) -> u8 {
    match ty {
        Type::Opaque(name) => class_of_name(name),
        Type::Vector(scalar, _) | Type::Scalar(scalar) => match scalar {
            crate::types::Scalar::Int => 1,
            crate::types::Scalar::Uint => 2,
            _ => 0,
        },
        _ => 0,
    }
}

fn class_of_name(name: &str) -> u8 {
    if name.starts_with("isampler") || name.starts_with("iimage") || name.starts_with("ivec") {
        1
    } else if name.starts_with("usampler")
        || name.starts_with("uimage")
        || name.starts_with("uvec")
    {
        2
    } else {
        0
    }
}
