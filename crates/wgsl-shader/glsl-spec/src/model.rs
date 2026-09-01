//! The shape of a builtin.
//!
//! Everything here is `'static` and `Copy`-cheap because every value of it is
//! a `static` produced by `glsl-spec-gen` and read from wasm. Three habits
//! follow from that and none of them are negotiable:
//!
//! - **Prose lives in one pool.** [`DocRef`] is an offset and a length into a
//!   single `&'static str`; a per-entry `&str` would cost two pointers of
//!   relocation apiece for ~500 entries and compress far worse.
//! - **Generic families stay symbolic.** [`TypeRef::Family`] names the spec's
//!   own notation (`genType`, `gvec4`, `gsampler2D`) and defers expansion to
//!   overload resolution, so hover can print what the reference page prints.
//! - **Tables are sorted by name** and read with binary search. The generator
//!   sorts; [`crate::function`] and friends assume it, and a test asserts it.

use crate::generated;
use crate::version::{Availability, DesktopMask, EsMask, Version};

/// How a parameter is passed, from the callee's point of view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum Flow {
    /// Read by the callee. The default; never written in a prototype.
    #[default]
    In,
    /// Written by the callee — the argument must be an lvalue.
    Out,
    /// Read and written.
    InOut,
}

impl Flow {
    /// The qualifier as a *prototype* writes it — empty for `in`, which
    /// prototypes leave implicit.
    pub const fn keyword(self) -> &'static str {
        match self {
            Flow::In => "",
            Flow::Out => "out",
            Flow::InOut => "inout",
        }
    }

    /// The qualifier as a *variable declaration* writes it, where `in` is
    /// spelled out because it is the half of the story that matters.
    pub const fn declared_keyword(self) -> &'static str {
        match self {
            Flow::In => "in",
            Flow::Out => "out",
            Flow::InOut => "inout",
        }
    }

    /// Whether the argument has to be assignable.
    pub const fn writes(self) -> bool {
        matches!(self, Flow::Out | Flow::InOut)
    }
}

/// A generic family in the spec's notation: the set of concrete types one
/// symbolic name stands for.
///
/// `genType` is `float vec2 vec3 vec4`; `gsampler2D` is
/// `sampler2D isampler2D usampler2D`. Overload resolution binds the family
/// once per call and requires every occurrence in that prototype to agree,
/// which is why the family is one value rather than expanded into a hundred
/// separate overloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Family {
    /// The spec's own spelling — what hover shows.
    pub name: &'static str,
    /// Every concrete type the name covers, in the spec's order.
    pub members: &'static [&'static str],
}

/// A family's index in [`generated::FAMILIES`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FamilyId(pub u16);

impl FamilyId {
    pub fn get(self) -> &'static Family {
        &generated::FAMILIES[self.0 as usize]
    }
}

/// A type as a prototype writes it: either a concrete GLSL type or a family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeRef {
    /// `vec4`, `float`, `sampler2DShadow`, `float[4]`.
    Concrete(&'static str),
    /// `genType`, `gvec4`, `gimage2DArray`.
    Family(FamilyId),
}

impl TypeRef {
    /// The spelling to print — the concrete name, or the family's own name.
    pub fn name(self) -> &'static str {
        match self {
            TypeRef::Concrete(name) => name,
            TypeRef::Family(id) => id.get().name,
        }
    }

    /// The family, when this is one.
    pub fn family(self) -> Option<&'static Family> {
        match self {
            TypeRef::Concrete(_) => None,
            TypeRef::Family(id) => Some(id.get()),
        }
    }

    /// Whether a concrete type spelling is one this reference admits. For a
    /// concrete reference that is equality; for a family, membership.
    pub fn accepts(self, concrete: &str) -> bool {
        match self {
            TypeRef::Concrete(name) => name == concrete,
            TypeRef::Family(id) => id.get().members.contains(&concrete),
        }
    }
}

/// A slice of the documentation pool, [`generated::DOCS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct DocRef {
    offset: u32,
    len: u32,
}

impl DocRef {
    /// No documentation. Reads back as the empty string.
    pub const EMPTY: DocRef = DocRef { offset: 0, len: 0 };

    /// Only the generator calls this. The bounds are its responsibility and
    /// [`crate::tests`] checks them for every entry.
    pub const fn new(offset: u32, len: u32) -> DocRef {
        DocRef { offset, len }
    }

    /// The markdown, ready for an LSP hover.
    pub fn text(self) -> &'static str {
        &generated::DOCS[self.offset as usize..(self.offset + self.len) as usize]
    }

    pub const fn is_empty(self) -> bool {
        self.len == 0
    }
}

/// A parameter's documentation, keyed by the name the prototypes use. Page
/// level rather than overload level, because the reference pages document a
/// parameter once for every overload that has it.
#[derive(Debug, Clone, Copy)]
pub struct ParamDoc {
    pub name: &'static str,
    pub doc: DocRef,
}

/// One parameter of one overload.
#[derive(Debug, Clone, Copy)]
pub struct Param {
    pub name: &'static str,
    pub ty: TypeRef,
    pub flow: Flow,
    /// Written `[float bias]` on the reference page — the texture-lookup bias
    /// and its relatives, which may be omitted at the call site.
    pub optional: bool,
}

/// One signature of a builtin function.
#[derive(Debug, Clone, Copy)]
pub struct Overload {
    pub ret: TypeRef,
    pub params: &'static [Param],
    /// Availability of *this signature*, which can be narrower than the
    /// function's: `mix(genDType)` arrived in 4.00 while `mix(genType)` has
    /// always been there. Where docs.gl's version table could not be matched to
    /// a signature this falls back to the function's own mask, so it is never
    /// wrong in the restrictive direction. See research/docs-gl.md §5.1.
    pub desktop: DesktopMask,
    pub es: EsMask,
}

impl Overload {
    pub const fn availability(&self) -> Availability {
        Availability::new(self.desktop, self.es)
    }

    /// The signature as GLSL would write it, for hover and signature help:
    /// `gvec4 texture(gsampler2D sampler, vec2 P, [float bias])`.
    pub fn signature(&self, name: &str) -> String {
        let mut out = String::with_capacity(32 + self.params.len() * 16);
        out.push_str(self.ret.name());
        out.push(' ');
        out.push_str(name);
        out.push('(');
        for (i, param) in self.params.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            if param.optional {
                out.push('[');
            }
            if param.flow != Flow::In {
                out.push_str(param.flow.keyword());
                out.push(' ');
            }
            out.push_str(param.ty.name());
            out.push(' ');
            out.push_str(param.name);
            if param.optional {
                out.push(']');
            }
        }
        out.push(')');
        out
    }

    /// How many arguments a call may pass, `(minimum, maximum)`. They differ
    /// only where the signature ends in optional parameters.
    pub fn arity(&self) -> (usize, usize) {
        let required = self.params.iter().filter(|p| !p.optional).count();
        (required, self.params.len())
    }
}

/// A builtin function: every overload the reference pages declare under one
/// name, with the page's prose.
#[derive(Debug, Clone, Copy)]
pub struct BuiltinFunction {
    pub name: &'static str,
    pub overloads: &'static [Overload],
    /// The description, as markdown.
    pub doc: DocRef,
    /// Per-parameter prose, sorted by name.
    pub param_docs: &'static [ParamDoc],
    /// The union of every overload's availability — the authoritative mask,
    /// and the one to answer "does this name exist here" with.
    pub desktop: DesktopMask,
    pub es: EsMask,
}

impl BuiltinFunction {
    pub const fn availability(&self) -> Availability {
        Availability::new(self.desktop, self.es)
    }

    /// Whether the name exists at all in this version.
    pub const fn available_in(&self, version: Version) -> bool {
        self.availability().contains(version)
    }

    /// The overloads this version actually has.
    pub fn overloads_in(
        &self,
        version: Version,
    ) -> impl Iterator<Item = &'static Overload> + use<> {
        let overloads = self.overloads;
        overloads.iter().filter(move |o| o.availability().contains(version))
    }

    pub fn param_doc(&self, name: &str) -> Option<DocRef> {
        self.param_docs
            .binary_search_by_key(&name, |p| p.name)
            .ok()
            .map(|i| self.param_docs[i].doc)
    }
}

/// A shader stage, in pipeline order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Stage {
    Vertex,
    TessControl,
    TessEvaluation,
    Geometry,
    Fragment,
    Compute,
}

impl Stage {
    pub const ALL: &'static [Stage] = &[
        Stage::Vertex,
        Stage::TessControl,
        Stage::TessEvaluation,
        Stage::Geometry,
        Stage::Fragment,
        Stage::Compute,
    ];

    /// What the reference pages and diagnostics call it.
    pub const fn label(self) -> &'static str {
        match self {
            Stage::Vertex => "vertex",
            Stage::TessControl => "tessellation control",
            Stage::TessEvaluation => "tessellation evaluation",
            Stage::Geometry => "geometry",
            Stage::Fragment => "fragment",
            Stage::Compute => "compute",
        }
    }

    const fn index(self) -> u32 {
        match self {
            Stage::Vertex => 0,
            Stage::TessControl => 1,
            Stage::TessEvaluation => 2,
            Stage::Geometry => 3,
            Stage::Fragment => 4,
            Stage::Compute => 5,
        }
    }
}

/// The stages a builtin variable exists in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct StageMask(u8);

impl StageMask {
    pub const EMPTY: StageMask = StageMask(0);
    pub const ALL: StageMask = StageMask((1 << 6) - 1);

    pub const fn from_bits(bits: u8) -> StageMask {
        StageMask(bits)
    }

    pub const fn bits(self) -> u8 {
        self.0
    }

    pub const fn contains(self, stage: Stage) -> bool {
        self.0 & (1 << stage.index()) != 0
    }

    pub const fn with(self, stage: Stage) -> StageMask {
        StageMask(self.0 | (1 << stage.index()))
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn stages(self) -> impl Iterator<Item = Stage> {
        Stage::ALL.iter().copied().filter(move |&s| self.contains(s))
    }
}

/// A predeclared `gl_*` variable.
#[derive(Debug, Clone, Copy)]
pub struct BuiltinVariable {
    pub name: &'static str,
    /// The declared type, array suffix included: `vec4`, `float[4]`, `int[]`.
    pub ty: &'static str,
    /// Where it exists. Never empty — a variable whose stage could not be read
    /// from the page gets every stage, because refusing a legal name is worse
    /// than accepting one in the wrong shader.
    pub stages: StageMask,
    /// `in` or `out` from the shader's point of view. Variables that are `in`
    /// in one stage and `out` in another (`gl_Layer`, `gl_PrimitiveID`) are
    /// recorded as [`Flow::InOut`].
    pub flow: Flow,
    pub doc: DocRef,
    pub desktop: DesktopMask,
    pub es: EsMask,
}

impl BuiltinVariable {
    pub const fn availability(&self) -> Availability {
        Availability::new(self.desktop, self.es)
    }

    pub const fn available_in(&self, version: Version) -> bool {
        self.availability().contains(version)
    }

    /// Whether the variable exists in this version *and* this stage.
    pub const fn available_at(&self, version: Version, stage: Stage) -> bool {
        self.available_in(version) && self.stages.contains(stage)
    }

    /// The declaration as GLSL would write it: `in vec4 gl_FragCoord`.
    pub fn declaration(&self) -> String {
        let keyword = self.flow.declared_keyword();
        let mut out = String::with_capacity(keyword.len() + self.ty.len() + self.name.len() + 3);
        out.push_str(keyword);
        out.push(' ');
        // The array suffix belongs after the name in GLSL, not after the type.
        match self.ty.split_once('[') {
            Some((base, suffix)) => {
                out.push_str(base);
                out.push(' ');
                out.push_str(self.name);
                out.push('[');
                out.push_str(suffix);
            }
            None => {
                out.push_str(self.ty);
                out.push(' ');
                out.push_str(self.name);
            }
        }
        out
    }
}
