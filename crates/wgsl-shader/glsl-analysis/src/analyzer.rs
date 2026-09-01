//! The walk: declarations, scopes and the state every rule reads (P4-01).
//!
//! Two passes over the tree, for one reason: an editor must answer for a file
//! being typed, and in a file being typed a function is called above where it
//! is defined all the time. Pass one records every file-scope declaration; pass
//! two walks the bodies with all of them already in scope. GLSL itself requires
//! declaration before use, so this is deliberately more permissive than the
//! language — a missed error beats a false one.
//!
//! Everything that can report is gated on [`Analyzer::may_error`]. A file whose
//! *parse* failed, whose preprocessor complained, or which pulls in an
//! `#include` we never followed, gets resolution and types — hover and
//! go-to-definition still work — and no error-severity diagnostics at all,
//! because the input the rules would be reading is not the input the user
//! wrote.

use analyzer_core::diagnostics::Severity;
use analyzer_core::spans::ByteSpan;
use glsl_syntax::{NodeId, NodeKind, Preprocessed, SyntaxTree, TokenId};

use crate::context::{Context, Options};
use crate::diagnostics::{SemanticCode, SemanticDiagnostic};
use crate::symbols::{
    Parameter, Qualifiers, Scopes, Signature, Symbol, SymbolId, SymbolKind, SymbolTable,
};
use crate::types::{Field, StructDef, StructId, StructTable, Type};
use crate::{Analysis, ResolvedRef, Target};

/// How deep the analysis will recurse before it calls an expression
/// pathological and answers `Unknown`.
///
/// The parser caps its own recursion at 64, but that cap counts *assignments*,
/// not precedence rungs: `1+1+1+…` a thousand terms long is one assignment and
/// a thousand nested `BinaryExpr`s. An arena cannot overflow the stack but a
/// recursive walk over one can, and "never panics" has to include this.
const MAX_DEPTH: u32 = 128;

/// Type names the analysis knows it does not know.
///
/// Vulkan GLSL and the ray-tracing, mesh and cooperative-matrix extensions add
/// opaque types by the dozen, none of which any table here carries. Naming one
/// is not an error; it is a type this analysis has no opinion about, and the
/// difference matters because the alternative is painting valid Vulkan shaders
/// red. Extension *suffixes* are handled separately by
/// [`looks_like_extension_name`].
const EXTENSION_TYPES: &[&str] = &[
    "sampler",
    "samplerShadow",
    "texture1D",
    "texture1DArray",
    "texture2D",
    "texture2DArray",
    "texture2DMS",
    "texture2DMSArray",
    "texture2DRect",
    "texture3D",
    "textureBuffer",
    "textureCube",
    "textureCubeArray",
    "itexture1D",
    "itexture1DArray",
    "itexture2D",
    "itexture2DArray",
    "itexture2DMS",
    "itexture2DMSArray",
    "itexture2DRect",
    "itexture3D",
    "itextureBuffer",
    "itextureCube",
    "itextureCubeArray",
    "utexture1D",
    "utexture1DArray",
    "utexture2D",
    "utexture2DArray",
    "utexture2DMS",
    "utexture2DMSArray",
    "utexture2DRect",
    "utexture3D",
    "utextureBuffer",
    "utextureCube",
    "utextureCubeArray",
    "subpassInput",
    "subpassInputMS",
    "isubpassInput",
    "isubpassInputMS",
    "usubpassInput",
    "usubpassInputMS",
    "coopmat",
    "coopvecNV",
    "tensorARM",
    "tensorLayoutNV",
    "tensorViewNV",
    "hitObjectNV",
    "spirv_type",
    // The half- and small-integer type families of GL_EXT_shader_16bit_storage
    // and friends, which are spelled like core types but are not.
    "float16_t",
    "float32_t",
    "float64_t",
    "int8_t",
    "int16_t",
    "int32_t",
    "int64_t",
    "uint8_t",
    "uint16_t",
    "uint32_t",
    "uint64_t",
];

/// Whether a name belongs to an extension rather than to core GLSL.
///
/// Two signals, both deliberately blunt: the `gl_` prefix, which the language
/// reserves and every extension uses, and the vendor suffixes every extension
/// type and builtin carries. A user identifier that ends in `EXT` will be left
/// alone by the unknown-name rules, which is a trade worth making.
pub(crate) fn looks_like_extension_name(name: &str) -> bool {
    const SUFFIXES: &[&str] = &[
        "EXT", "NV", "ARB", "AMD", "OES", "KHR", "QCOM", "IMG", "INTEL", "MESA", "SGIS",
        "SGIX", "APPLE", "HUAWEI", "ARM", "NVX", "AMDX", "VALVE",
    ];
    if name.starts_with("gl_") || name.starts_with("__") {
        return true;
    }
    // A vendor suffix only counts on a name long enough to have one, so a
    // variable called `nv` or `arm` is still an ordinary name.
    SUFFIXES.iter().any(|suffix| name.len() > suffix.len() + 1 && name.ends_with(suffix))
        || EXTENSION_TYPES.contains(&name)
        || name.starts_with("f16")
        || name.starts_with("f32")
        || name.starts_with("f64")
        || name.starts_with("i8")
        || name.starts_with("i16")
        || name.starts_with("i64")
        || name.starts_with("u8")
        || name.starts_with("u16")
        || name.starts_with("u64")
        || name.starts_with("spirv_")
}

/// Everything the walk carries.
pub(crate) struct Analyzer<'a> {
    pub tree: &'a SyntaxTree,
    pub pp: &'a Preprocessed,
    pub ctx: Context,
    pub structs: StructTable,
    pub symbols: SymbolTable,
    pub scopes: Scopes,
    pub diagnostics: Vec<SemanticDiagnostic>,
    /// One entry per tree node: the type of the expression there, or `Unknown`.
    pub types: Vec<Type>,
    pub refs: Vec<ResolvedRef>,
    /// Whether the input is trustworthy enough to report errors about.
    pub strict: bool,
    /// The return type of the function being walked.
    pub return_type: Type,
    pub loop_depth: u32,
    pub switch_depth: u32,
    pub depth: u32,
}

impl<'a> Analyzer<'a> {
    pub fn new(
        tree: &'a SyntaxTree,
        pp: &'a Preprocessed,
        options: &Options,
    ) -> Analyzer<'a> {
        let ctx = Context::new(pp, options);
        let parse_clean = !tree.diagnostics.iter().any(|d| d.severity == Severity::Error);
        let pp_clean = !pp.diagnostics.iter().any(|d| d.severity == Severity::Error);
        Analyzer {
            tree,
            pp,
            ctx,
            structs: StructTable::default(),
            symbols: SymbolTable::default(),
            scopes: Scopes::default(),
            diagnostics: Vec::new(),
            types: vec![Type::Unknown; tree.node_count()],
            refs: Vec::new(),
            // An `#include` we never followed is a file whose other half is
            // missing; every unknown name in it may well be declared there.
            strict: parse_clean && pp_clean && pp.directives.includes.is_empty(),
            return_type: Type::Void,
            loop_depth: 0,
            switch_depth: 0,
            depth: 0,
        }
    }

    // -- reporting ---------------------------------------------------------

    /// Whether an error-severity diagnostic may be reported at all.
    pub fn may_error(&self) -> bool {
        self.strict
    }

    pub fn error(&mut self, code: SemanticCode, message: impl Into<String>, span: ByteSpan) {
        if !self.may_error() {
            return;
        }
        self.diagnostics.push(SemanticDiagnostic::error(code, message, span));
    }

    pub fn warn(&mut self, code: SemanticCode, message: impl Into<String>, span: ByteSpan) {
        self.diagnostics.push(SemanticDiagnostic::warning(code, message, span));
    }

    // -- reading the tree --------------------------------------------------

    /// The first token of a node, when it has one.
    pub fn first_token(&self, node: NodeId) -> Option<TokenId> {
        let range = self.tree.node(node).tokens;
        (range.0.0 < range.1.0).then_some(range.0)
    }

    pub fn token_text(&self, token: TokenId) -> &'a str {
        self.pp.tokens.get(token.index()).map_or("", |t| t.text.as_str())
    }

    /// The spelling of a one-token node — a `Name`, a `NameExpr`, a type token.
    pub fn node_text(&self, node: NodeId) -> &'a str {
        self.first_token(node).map_or("", |t| self.token_text(t))
    }

    pub fn span(&self, node: NodeId) -> ByteSpan {
        self.tree.span(node)
    }

    /// The `Name` child of a declaration, as text and span.
    pub fn name_of(&self, node: NodeId) -> Option<(&'a str, ByteSpan)> {
        let name = self.tree.child_of_kind(node, NodeKind::Name)?;
        Some((self.node_text(name), self.tree.span(name)))
    }

    /// Record what an identifier occurrence resolved to.
    pub fn record(&mut self, span: ByteSpan, target: Target) {
        self.refs.push(ResolvedRef { span, target });
    }

    pub fn set_type(&mut self, node: NodeId, ty: Type) {
        if let Some(slot) = self.types.get_mut(node.index()) {
            *slot = ty;
        }
    }

    // -- types -------------------------------------------------------------

    /// The type a `TypeSpec` node names, array suffixes included.
    pub fn type_of_spec(&mut self, spec: NodeId) -> Type {
        let mut base = match self.tree.child_of_kind(spec, NodeKind::StructSpec) {
            Some(struct_spec) => self.declare_struct(struct_spec),
            None => {
                let Some(token) = self.first_token(spec) else {
                    return Type::Unknown;
                };
                let name = self.token_text(token);
                let span = self
                    .pp
                    .tokens
                    .get(token.index())
                    .map_or(self.span(spec), |t| t.span);
                self.named_type(name, span)
            }
        };
        // `float[2][3] x` — the leftmost suffix is the outermost dimension, so
        // the wrapping runs right to left.
        let sizes = self.array_sizes(spec);
        for size in sizes.into_iter().rev() {
            base = Type::Array(Box::new(base), size);
        }
        base
    }

    /// The type a name spells: a basic type, a declared struct, or nothing.
    pub fn named_type(&mut self, name: &str, span: ByteSpan) -> Type {
        if let Some(ty) = Type::from_name(name) {
            self.record(span, Target::Type(ty.clone()));
            return ty;
        }
        if let Some(id) = self.scopes.lookup_one(name) {
            if let Some(symbol) = self.symbols.get(id) {
                if matches!(symbol.kind, SymbolKind::Struct | SymbolKind::Block) {
                    let ty = symbol.ty.clone();
                    self.record(span, Target::Symbol(id));
                    return ty;
                }
            }
        }
        if !looks_like_extension_name(name) && self.names_trusted() {
            self.error(
                SemanticCode::UnknownType,
                format!("'{name}' is not a type"),
                span,
            );
        }
        self.record(span, Target::Unresolved);
        Type::Unknown
    }

    /// Every `[ … ]` directly under a node, as sizes. `None` is an unsized one.
    pub fn array_sizes(&mut self, node: NodeId) -> Vec<Option<u32>> {
        let specs: Vec<NodeId> = self
            .tree
            .child_nodes(node)
            .filter(|c| self.tree.kind(*c) == NodeKind::ArraySpec)
            .collect();
        specs.into_iter().map(|spec| self.array_size(spec)).collect()
    }

    /// One `[ … ]`: its size, and the diagnostics for a size that cannot be one.
    fn array_size(&mut self, spec: NodeId) -> Option<u32> {
        let expr = self.tree.child_nodes(spec).find(|c| self.tree.kind(*c).is_expression())?;
        let value = crate::consteval::const_int(self, expr);
        // Walk it anyway so the names inside it resolve for the editor.
        self.expression(expr);
        match value {
            Some(size) if size > 0 => u32::try_from(size).ok(),
            Some(size) => {
                let span = self.span(expr);
                self.error(
                    SemanticCode::BadArraySize,
                    format!("an array size must be greater than zero, and this is {size}"),
                    span,
                );
                None
            }
            None => {
                if crate::consteval::certainly_not_constant(self, expr) {
                    let span = self.span(expr);
                    self.error(
                        SemanticCode::BadArraySize,
                        "an array size must be a constant expression",
                        span,
                    );
                }
                None
            }
        }
    }

    // -- declarations ------------------------------------------------------

    /// Add a symbol to the current scope, reporting a clash.
    pub fn declare(&mut self, symbol: Symbol) -> SymbolId {
        let name = symbol.name.clone();
        let kind = symbol.kind;
        let span = symbol.name_span;
        let id = self.symbols.push(symbol);
        // The declaration site is an occurrence like any other, so rename and
        // go-to-definition answer from the name itself as well as from a use.
        self.record(span, Target::Symbol(id));
        if kind == SymbolKind::Field {
            // Members are reached through their owner, never through a scope.
            return id;
        }
        let overloadable = kind == SymbolKind::Function;
        let clash = self.scopes.declare(&name, id, overloadable);
        // A `gl_`-prefixed name is one the language or an extension may already
        // predeclare, and redeclaring `gl_PerVertex` is how a shader says what
        // it uses. Never a clash worth reporting.
        if let Some(_previous) = clash {
            // An interface block's *name* is not a variable: `in Primitive {…}`
            // and `out Primitive {…}` in one shader is how a stage declares
            // both ends of an interface, and it is not a redeclaration.
            if !looks_like_extension_name(&name) && kind != SymbolKind::Block {
                self.error(
                    SemanticCode::Redeclaration,
                    format!("'{name}' is already declared in this scope"),
                    span,
                );
            }
        }
        id
    }

    /// `struct Name { … }` — registers the type and, when it has a name, the
    /// symbol that names it.
    pub fn declare_struct(&mut self, spec: NodeId) -> Type {
        let decl_span = self.span(spec);
        // Read twice — a return type is read when the signature is collected
        // and again when the body is walked — is still one struct.
        if let Some(existing) = self.structs.by_declaration(decl_span) {
            return Type::Struct(existing);
        }
        let (name, name_span) = match self.name_of(spec) {
            Some((name, span)) => (name.to_string(), span),
            None => (String::new(), decl_span),
        };
        let id = self.structs.push(StructDef {
            name: if name.is_empty() { "<anonymous>".to_string() } else { name.clone() },
            fields: Vec::new(),
            is_block: false,
            decl_span,
        });
        let fields = match self.tree.child_of_kind(spec, NodeKind::FieldList) {
            Some(list) => self.fields_of(list, Some(id)),
            None => Vec::new(),
        };
        if let Some(def) = self.structs.get_mut(id) {
            def.fields = fields;
        }
        if !name.is_empty() {
            self.declare(Symbol {
                name,
                kind: SymbolKind::Struct,
                ty: Type::Struct(id),
                qualifiers: Qualifiers::default(),
                name_span,
                full_span: self.span(spec),
                signature: None,
                struct_id: Some(id),
                is_prototype: false,
                const_value: None,
            });
        }
        Type::Struct(id)
    }

    /// The members of a `FieldList`, in order. `owner` is the struct they
    /// belong to, when they belong to one.
    pub fn fields_of(&mut self, list: NodeId, owner: Option<StructId>) -> Vec<Field> {
        let decls: Vec<NodeId> = self
            .tree
            .child_nodes(list)
            .filter(|c| self.tree.kind(*c) == NodeKind::FieldDecl)
            .collect();
        let mut fields: Vec<Field> = Vec::new();
        for decl in decls {
            let Some(spec) = self.tree.child_of_kind(decl, NodeKind::TypeSpec) else {
                continue;
            };
            let base = self.type_of_spec(spec);
            let declarators: Vec<NodeId> = self
                .tree
                .child_nodes(decl)
                .filter(|c| self.tree.kind(*c) == NodeKind::Declarator)
                .collect();
            for declarator in declarators {
                let Some((name, name_span)) = self.name_of(declarator) else {
                    continue;
                };
                let ty = self.declarator_type(declarator, &base);
                if fields.iter().any(|f| f.name == name) && !looks_like_extension_name(name) {
                    self.error(
                        SemanticCode::Redeclaration,
                        format!("'{name}' is already a member here"),
                        name_span,
                    );
                }
                fields.push(Field { name: name.to_string(), ty: ty.clone(), name_span });
                if let Some(owner) = owner {
                    self.symbols.push(Symbol {
                        name: name.to_string(),
                        kind: SymbolKind::Field,
                        ty,
                        qualifiers: Qualifiers::default(),
                        name_span,
                        full_span: self.span(declarator),
                        signature: None,
                        struct_id: Some(owner),
                        is_prototype: false,
                        const_value: None,
                    });
                }
            }
        }
        fields
    }

    /// A declarator's own type: the declaration's type plus its `[ … ]`
    /// suffixes, which bind to the name rather than to the type.
    pub fn declarator_type(&mut self, declarator: NodeId, base: &Type) -> Type {
        let sizes = self.array_sizes(declarator);
        let mut ty = base.clone();
        for size in sizes.into_iter().rev() {
            ty = Type::Array(Box::new(ty), size);
        }
        ty
    }

    /// The qualifiers a declaration carries.
    pub fn qualifiers_of(&self, node: NodeId) -> Qualifiers {
        let mut qualifiers = Qualifiers::default();
        let Some(list) = self.tree.child_of_kind(node, NodeKind::QualifierList) else {
            return qualifiers;
        };
        for token in self.tree.leaves(list) {
            match self.token_text(token) {
                "const" => qualifiers.is_const = true,
                "uniform" => qualifiers.is_uniform = true,
                "buffer" => qualifiers.is_buffer = true,
                "shared" => qualifiers.is_shared = true,
                "in" | "attribute" => qualifiers.is_in = true,
                "out" => qualifiers.is_out = true,
                "inout" => {
                    qualifiers.is_in = true;
                    qualifiers.is_out = true;
                }
                "varying" => {
                    qualifiers.is_varying = true;
                    qualifiers.is_in = true;
                }
                _ => {}
            }
        }
        qualifiers
    }

    // -- the two passes ----------------------------------------------------

    /// Everything at file scope, without entering a body.
    pub fn collect_globals(&mut self) {
        let children: Vec<NodeId> = self.tree.child_nodes(self.tree.root()).collect();
        for child in children {
            match self.tree.kind(child) {
                NodeKind::Declaration => self.global_declaration(child),
                NodeKind::FunctionDecl => self.declare_function(child),
                NodeKind::InterfaceBlock => self.interface_block(child),
                _ => {}
            }
        }
    }

    /// `uniform mat4 view, proj[2];` at file scope. Initialisers are checked in
    /// the second pass, when every global is already in scope.
    fn global_declaration(&mut self, node: NodeId) {
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
            let Some((name, name_span)) = self.name_of(declarator) else {
                continue;
            };
            let ty = self.declarator_type(declarator, &base);
            // A `const int` folds now rather than in the second pass, because
            // the array sizes that read it are resolved in this one.
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
                kind: SymbolKind::Global,
                ty,
                qualifiers,
                name_span,
                full_span: self.span(declarator),
                signature: None,
                struct_id: None,
                is_prototype: false,
                const_value,
            });
        }
    }

    /// The function's name, signature and return type. Not its body.
    fn declare_function(&mut self, node: NodeId) {
        let Some((name, name_span)) = self.name_of(node) else {
            return;
        };
        let ret = match self.tree.child_of_kind(node, NodeKind::TypeSpec) {
            Some(spec) => self.type_of_spec(spec),
            None => Type::Unknown,
        };
        let params = self.parameters_of(node);
        let has_body = self.tree.child_of_kind(node, NodeKind::CompoundStmt).is_some();
        self.declare(Symbol {
            name: name.to_string(),
            kind: SymbolKind::Function,
            ty: ret.clone(),
            qualifiers: Qualifiers::default(),
            name_span,
            full_span: self.span(node),
            signature: Some(Signature { ret, params }),
            struct_id: None,
            is_prototype: !has_body,
            const_value: None,
        });
    }

    /// The parameter list of a function declaration, as the signature wants it.
    pub fn parameters_of(&mut self, function: NodeId) -> Vec<Parameter> {
        let Some(list) = self.tree.child_of_kind(function, NodeKind::ParameterList) else {
            return Vec::new();
        };
        let params: Vec<NodeId> = self
            .tree
            .child_nodes(list)
            .filter(|c| self.tree.kind(*c) == NodeKind::Parameter)
            .collect();
        let mut out = Vec::with_capacity(params.len());
        for param in params {
            let qualifiers = self.qualifiers_of(param);
            let Some(spec) = self.tree.child_of_kind(param, NodeKind::TypeSpec) else {
                continue;
            };
            let base = self.type_of_spec(spec);
            // `void f(void)` declares no parameter at all.
            if base == Type::Void {
                continue;
            }
            let (name, ty) = match self.tree.child_of_kind(param, NodeKind::Declarator) {
                Some(declarator) => {
                    let ty = self.declarator_type(declarator, &base);
                    let name = self.name_of(declarator).map(|(n, _)| n.to_string());
                    (name.unwrap_or_default(), ty)
                }
                None => (String::new(), base),
            };
            out.push(Parameter { name, ty, writes: qualifiers.is_out });
        }
        out
    }

    /// `layout(std140) uniform Camera { … } camera;`
    fn interface_block(&mut self, node: NodeId) {
        let Some((name, name_span)) = self.name_of(node) else {
            return;
        };
        let qualifiers = self.qualifiers_of(node);
        let id = self.structs.push(StructDef {
            name: name.to_string(),
            fields: Vec::new(),
            is_block: true,
            decl_span: self.span(node),
        });
        let instance =
            self.tree.child_nodes(node).find(|c| self.tree.kind(*c) == NodeKind::Declarator);
        let fields = match self.tree.child_of_kind(node, NodeKind::FieldList) {
            Some(list) => self.fields_of(list, instance.map(|_| id)),
            None => Vec::new(),
        };
        if let Some(def) = self.structs.get_mut(id) {
            def.fields = fields.clone();
        }
        self.declare(Symbol {
            name: name.to_string(),
            kind: SymbolKind::Block,
            ty: Type::Struct(id),
            qualifiers,
            name_span,
            full_span: self.span(node),
            signature: None,
            struct_id: Some(id),
            is_prototype: false,
            const_value: None,
        });
        match instance {
            // With an instance name the members are reached through it.
            Some(declarator) => {
                let Some((instance_name, instance_span)) = self.name_of(declarator) else {
                    return;
                };
                let ty = self.declarator_type(declarator, &Type::Struct(id));
                self.declare(Symbol {
                    name: instance_name.to_string(),
                    kind: SymbolKind::Global,
                    ty,
                    qualifiers,
                    name_span: instance_span,
                    full_span: self.span(declarator),
                    signature: None,
                    struct_id: Some(id),
                    is_prototype: false,
                    const_value: None,
                });
            }
            // Without one, GLSL puts the members in global scope.
            None => {
                for field in fields {
                    self.declare(Symbol {
                        name: field.name.clone(),
                        kind: SymbolKind::Global,
                        ty: field.ty.clone(),
                        qualifiers,
                        name_span: field.name_span,
                        full_span: field.name_span,
                        signature: None,
                        struct_id: None,
                        is_prototype: false,
                        const_value: None,
                    });
                }
            }
        }
    }

    /// Pass two: the initialisers of every global, then every function body.
    pub fn walk_bodies(&mut self) {
        let children: Vec<NodeId> = self.tree.child_nodes(self.tree.root()).collect();
        for child in children {
            match self.tree.kind(child) {
                NodeKind::Declaration => self.check_initialisers(child, true),
                NodeKind::FunctionDecl => self.function_body(child),
                _ => {}
            }
        }
    }

    /// The `= …` of each declarator, checked against the declared type.
    pub fn check_initialisers(&mut self, node: NodeId, global: bool) {
        let qualifiers = self.qualifiers_of(node);
        let declarators: Vec<NodeId> = self
            .tree
            .child_nodes(node)
            .filter(|c| self.tree.kind(*c) == NodeKind::Declarator)
            .collect();
        for declarator in declarators {
            let Some((name, name_span)) = self.name_of(declarator) else {
                continue;
            };
            let declared = self.declared_type_of(name, name_span, global);
            let initializer = self
                .tree
                .child_of_kind(declarator, NodeKind::Initializer)
                .and_then(|init| {
                    self.tree.child_nodes(init).find(|c| {
                        self.tree.kind(*c).is_expression()
                            || self.tree.kind(*c) == NodeKind::InitializerList
                    })
                });
            match initializer {
                Some(expr) => {
                    if self.tree.kind(expr) == NodeKind::InitializerList {
                        self.initializer_list(expr);
                        continue;
                    }
                    let value = self.expression(expr);
                    if qualifiers.is_const
                        && value.constant == crate::expr::Const::No
                        && !declared.is_unknown()
                    {
                        let span = self.span(expr);
                        self.error(
                            SemanticCode::ConstInitializer,
                            format!("'{name}' is const, so its value must be a constant \
                                     expression"),
                            span,
                        );
                    }
                    if !declared.is_unknown()
                        && !value.ty.is_unknown()
                        && !crate::conversions::implicitly_convertible(&value.ty, &declared)
                    {
                        let span = self.span(expr);
                        let (from, to) =
                            (value.ty.name(&self.structs), declared.name(&self.structs));
                        self.error(
                            SemanticCode::TypeMismatch,
                            format!("'{name}' is a {to} and this initialiser is a {from}"),
                            span,
                        );
                    }
                }
                None if qualifiers.is_const && !qualifiers.is_uniform => {
                    self.error(
                        SemanticCode::ConstInitializer,
                        format!("'{name}' is const and has no value"),
                        name_span,
                    );
                }
                None => {}
            }
        }
    }

    /// Walk a braced initialiser list. Its shape against the declared type is
    /// deliberately not checked: 4.20's rules are subtle and the payoff is a
    /// diagnostic nobody is asking for.
    fn initializer_list(&mut self, list: NodeId) {
        let children: Vec<NodeId> = self.tree.child_nodes(list).collect();
        for child in children {
            if self.tree.kind(child) == NodeKind::InitializerList {
                self.initializer_list(child);
            } else if self.tree.kind(child).is_expression() {
                self.expression(child);
            }
        }
    }

    /// The type the symbol table gave a name, for checking its own initialiser.
    fn declared_type_of(&mut self, name: &str, span: ByteSpan, global: bool) -> Type {
        let _ = global;
        match self.scopes.lookup_one(name) {
            Some(id) => {
                self.record(span, Target::Symbol(id));
                self.symbols.get(id).map_or(Type::Unknown, |s| s.ty.clone())
            }
            None => Type::Unknown,
        }
    }

    /// A function definition: its parameters, then its body.
    fn function_body(&mut self, node: NodeId) {
        let Some(body) = self.tree.child_of_kind(node, NodeKind::CompoundStmt) else {
            return;
        };
        let name = self.name_of(node).map(|(n, _)| n.to_string()).unwrap_or_default();
        let ret = match self.tree.child_of_kind(node, NodeKind::TypeSpec) {
            Some(spec) => self.type_of_spec(spec),
            None => Type::Unknown,
        };
        self.return_type = ret.clone();
        self.loop_depth = 0;
        self.switch_depth = 0;
        self.scopes.push();

        if let Some(list) = self.tree.child_of_kind(node, NodeKind::ParameterList) {
            let params: Vec<NodeId> = self
                .tree
                .child_nodes(list)
                .filter(|c| self.tree.kind(*c) == NodeKind::Parameter)
                .collect();
            for param in params {
                let qualifiers = self.qualifiers_of(param);
                let Some(spec) = self.tree.child_of_kind(param, NodeKind::TypeSpec) else {
                    continue;
                };
                let base = self.type_of_spec(spec);
                let Some(declarator) = self.tree.child_of_kind(param, NodeKind::Declarator)
                else {
                    continue;
                };
                let Some((param_name, param_span)) = self.name_of(declarator) else {
                    continue;
                };
                let ty = self.declarator_type(declarator, &base);
                self.declare(Symbol {
                    name: param_name.to_string(),
                    kind: SymbolKind::Parameter,
                    ty,
                    qualifiers,
                    name_span: param_span,
                    full_span: self.span(param),
                    signature: None,
                    struct_id: None,
                    is_prototype: false,
                    const_value: None,
                });
            }
        }

        let returns = self.compound(body);
        self.scopes.pop();

        if !matches!(ret, Type::Void | Type::Unknown) && !returns {
            let span = self
                .name_of(node)
                .map(|(_, span)| span)
                .unwrap_or_else(|| self.span(node));
            self.warn(
                SemanticCode::MissingReturn,
                format!(
                    "'{name}' returns {} and can reach its end without a value",
                    ret.name(&self.structs)
                ),
                span,
            );
        }
    }

    /// Run `f` one level deeper, answering `None` when the tree nests deeper
    /// than this walk recurses.
    pub fn deeper<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> Option<T> {
        if self.depth >= MAX_DEPTH {
            return None;
        }
        self.depth += 1;
        let value = f(self);
        self.depth -= 1;
        Some(value)
    }

    /// The finished answer.
    pub fn finish(mut self) -> Analysis {
        self.refs.sort_by_key(|r| (r.span.start, r.span.end));
        self.refs.dedup_by_key(|r| r.span);
        self.diagnostics.sort_by_key(|d| (d.span.start, d.span.end));
        // A type specifier can be read twice — a function's return type is read
        // once for its signature and again for its body — and the second read
        // must not double the first's diagnostics.
        self.diagnostics
            .dedup_by(|a, b| a.code == b.code && a.span == b.span && a.message == b.message);
        Analysis {
            context: self.ctx,
            structs: self.structs,
            symbols: self.symbols,
            diagnostics: self.diagnostics,
            types: self.types,
            references: self.refs,
        }
    }
}
