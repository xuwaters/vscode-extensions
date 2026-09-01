//! The predeclared surface docs.gl does not document.
//!
//! Hand-written from the specification text, beside [`crate::keywords`] and for
//! the same reason: the reference pages cover the *core* builtin functions and
//! variables of GLSL 4.x and ES 3.x and nothing else — not one page mentions
//! `gl_FragColor`, `texture2D`, `gl_TexCoord` or the builtin constants
//! ([research/docs-gl.md](../../../../docs/rfc/012-glsl-analyzer/research/docs-gl.md)
//! §8). Generating from docs.gl and stopping there would leave every WebGL1-era
//! and every compatibility-profile shader with an analyzer that cannot name
//! half of what it reads.
//!
//! Three groups, and only the first is really "legacy":
//!
//! 1. the **compatibility and ES 1.00** builtins — `gl_FragColor`, `texture2D`,
//!    the fixed-function matrices and varyings;
//! 2. the **builtin constants** of 4.60 §7.3, which are current in every
//!    version and simply have no page;
//! 3. the **4.60 arrivals** docs.gl's tables stop short of — the atomic-counter
//!    operations and the group-vote functions.
//!
//! Scope is corpus-driven, and the included and excluded sets are recorded in
//! [decision 0007](../../../../docs/rfc/012-glsl-analyzer/decisions/0007-legacy-builtin-table.md).
//! Sources: OpenGL Shading Language 1.20 §7 and §8, 4.60 §7.3, §8.10 and §8.18,
//! and GLSL ES 1.00 §7 and §8.
//!
//! Availability is expressed twice, because the compatibility profile is not a
//! version:
//!
//! - the masks say where the name is in **core** — `texture2D` is core desktop
//!   1.10 through 1.50 and core ES 1.00, and is gone from `#version 300 es` and
//!   from every desktop core profile;
//! - [`LegacyFunction::compatibility`] says the name survives in
//!   `#version N compatibility`, at any version.

use crate::model::{BuiltinFunction, BuiltinVariable, DocRef, Flow, Overload, Param, Stage,
    StageMask, TypeRef};
use crate::version::{DesktopMask, DesktopVersion, EsMask, EsVersion};

/// A builtin function the compatibility profile keeps and core has dropped.
#[derive(Debug, Clone, Copy)]
pub struct LegacyFunction {
    /// The same shape the generated tables use, so overload resolution needs no
    /// second code path.
    pub function: BuiltinFunction,
    /// A hand-written one-liner. The reference pages carry no prose for these,
    /// and inventing Khronos text would be worse than writing our own.
    pub doc: &'static str,
    /// Whether `#version N compatibility` keeps the name past the core mask.
    pub compatibility: bool,
}

/// A predeclared variable or constant the generated tables do not carry.
#[derive(Debug, Clone, Copy)]
pub struct LegacyVariable {
    pub variable: BuiltinVariable,
    pub doc: &'static str,
    pub compatibility: bool,
}

// -- masks ----------------------------------------------------------------

/// Core desktop GLSL 1.10 through 1.50 — every version before GLSL renumbered
/// itself to match GL.
///
/// The window ends there because that is where the language really changed
/// hands: GL 3.0 *deprecated* this surface and GL 3.2/GLSL 1.50 was the last
/// release before the core profile dropped it, so `gl_FragColor` and
/// `texture2D` are still accepted at 1.30, 1.40 and 1.50 — glslang accepts
/// them, and the corpus is full of valid shaders that use them there. From 3.30
/// on they exist only in the compatibility profile, which is the `compat` flag.
const D_LEGACY: DesktopMask = DesktopMask::through(DesktopVersion::V110, DesktopVersion::V150);
/// Every desktop version. What the builtin *constants* get: they are current.
const D_ALL: DesktopMask = DesktopMask::ALL;
/// GLSL 4.60 onward — where docs.gl's tables stop.
const D460: DesktopMask = DesktopMask::since(DesktopVersion::V460);
/// ES 1.00 only — the WebGL1 language.
const E_LEGACY: EsMask = EsMask::only(EsVersion::V100);
const E_ALL: EsMask = EsMask::ALL;
const E_NONE: EsMask = EsMask::EMPTY;

const VERTEX: StageMask = StageMask::EMPTY.with(Stage::Vertex);
const FRAGMENT: StageMask = StageMask::EMPTY.with(Stage::Fragment);
const VERTEX_GEOMETRY: StageMask = VERTEX.with(Stage::Geometry);
/// The stages a varying passes through: written by a vertex or geometry shader,
/// read by a fragment shader.
const VARYING: StageMask = VERTEX_GEOMETRY.with(Stage::Fragment);

// -- builders -------------------------------------------------------------

/// A required parameter.
const fn p(name: &'static str, ty: &'static str) -> Param {
    Param { name, ty: TypeRef::Concrete(ty), flow: Flow::In, optional: false }
}

/// An optional trailing parameter — the LOD bias every legacy fragment-stage
/// texture lookup takes.
const fn opt(name: &'static str, ty: &'static str) -> Param {
    Param { name, ty: TypeRef::Concrete(ty), flow: Flow::In, optional: true }
}

/// One overload, available wherever its function is.
const fn o(
    ret: &'static str,
    params: &'static [Param],
    desktop: DesktopMask,
    es: EsMask,
) -> Overload {
    Overload { ret: TypeRef::Concrete(ret), params, desktop, es }
}

macro_rules! legacy_functions {
    ($($name:literal, $desktop:expr, $es:expr, $compat:literal, $doc:literal,
        $overloads:expr;)+) => {
        /// Every legacy builtin function, in name order.
        pub static LEGACY_FUNCTIONS: &[LegacyFunction] = &[$(LegacyFunction {
            function: BuiltinFunction {
                name: $name,
                overloads: $overloads,
                doc: DocRef::EMPTY,
                param_docs: &[],
                desktop: $desktop,
                es: $es,
            },
            doc: $doc,
            compatibility: $compat,
        }),+];
    };
}

macro_rules! legacy_variables {
    ($($name:literal, $ty:literal, $stages:expr, $flow:ident, $desktop:expr, $es:expr,
        $compat:literal, $doc:literal;)+) => {
        /// Every predeclared variable and constant the generated tables miss:
        /// the legacy names first, then the builtin constants of §7.3, which
        /// are current in every version and simply undocumented.
        pub static LEGACY_VARIABLES: &[LegacyVariable] = &[$(LegacyVariable {
            variable: BuiltinVariable {
                name: $name,
                ty: $ty,
                stages: $stages,
                flow: Flow::$flow,
                doc: DocRef::EMPTY,
                desktop: $desktop,
                es: $es,
            },
            doc: $doc,
            compatibility: $compat,
        }),+];
    };
}

// -- the functions --------------------------------------------------------
//
// Two groups. The 4.60 arrivals docs.gl never got a page for, and the
// texture-lookup family of GLSL 1.20 §8.7 and GLSL ES 1.00 §8.4 — `texture2D`
// and friends, replaced by the overloaded `texture` in desktop 1.30 and ES
// 3.00. The `Proj` forms divide by the last component; the `Lod` forms are
// vertex-stage-only in ES; the optional `bias` parameter is fragment-stage
// only, which this table does not encode, because refusing a legal call is
// worse than accepting a doubtful one.

legacy_functions! {
    // ── Current, and documented nowhere ───────────────────────────────────
    // GLSL 4.60 §8.10 and §8.18. docs.gl stops at 4.50, so the builtins that
    // arrived with 4.60 — the atomic-counter operations of
    // ARB_shader_atomic_counter_ops and the group-vote functions of
    // ARB_shader_group_vote — have no page at all. They are not legacy; they
    // are simply undocumented, and the alternative to naming them here is an
    // analyzer that calls valid 4.60 shaders broken.
    "allInvocations", D460, E_NONE, false,
        "Whether the expression is true for every active invocation in the group.",
        &[o("bool", &[p("value", "bool")], D460, E_NONE)];
    "allInvocationsEqual", D460, E_NONE, false,
        "Whether the expression has the same value in every active invocation.",
        &[o("bool", &[p("value", "bool")], D460, E_NONE)];
    "anyInvocation", D460, E_NONE, false,
        "Whether the expression is true for any active invocation in the group.",
        &[o("bool", &[p("value", "bool")], D460, E_NONE)];
    "atomicCounterAdd", D460, E_NONE, false,
        "Atomically adds to an atomic counter and answers its value beforehand.",
        &[o("uint", &[p("c", "atomic_uint"), p("data", "uint")], D460, E_NONE)];
    "atomicCounterAnd", D460, E_NONE, false,
        "Atomically ands an atomic counter and answers its value beforehand.",
        &[o("uint", &[p("c", "atomic_uint"), p("data", "uint")], D460, E_NONE)];
    "atomicCounterCompSwap", D460, E_NONE, false,
        "Atomically compares and swaps an atomic counter, answering its value \
         beforehand.",
        &[o("uint", &[p("c", "atomic_uint"), p("compare", "uint"), p("data", "uint")],
            D460, E_NONE)];
    "atomicCounterExchange", D460, E_NONE, false,
        "Atomically replaces an atomic counter and answers its value beforehand.",
        &[o("uint", &[p("c", "atomic_uint"), p("data", "uint")], D460, E_NONE)];
    "atomicCounterMax", D460, E_NONE, false,
        "Atomically takes the maximum of an atomic counter and a value.",
        &[o("uint", &[p("c", "atomic_uint"), p("data", "uint")], D460, E_NONE)];
    "atomicCounterMin", D460, E_NONE, false,
        "Atomically takes the minimum of an atomic counter and a value.",
        &[o("uint", &[p("c", "atomic_uint"), p("data", "uint")], D460, E_NONE)];
    "atomicCounterOr", D460, E_NONE, false,
        "Atomically ors an atomic counter and answers its value beforehand.",
        &[o("uint", &[p("c", "atomic_uint"), p("data", "uint")], D460, E_NONE)];
    "atomicCounterSubtract", D460, E_NONE, false,
        "Atomically subtracts from an atomic counter and answers its value \
         beforehand.",
        &[o("uint", &[p("c", "atomic_uint"), p("data", "uint")], D460, E_NONE)];
    "atomicCounterXor", D460, E_NONE, false,
        "Atomically xors an atomic counter and answers its value beforehand.",
        &[o("uint", &[p("c", "atomic_uint"), p("data", "uint")], D460, E_NONE)];

    // ── The compatibility and ES 1.00 surface ─────────────────────────────
    "ftransform", D_LEGACY, E_NONE, true,
        "The fixed-function vertex transform: `gl_ModelViewProjectionMatrix * gl_Vertex`, \
         computed the way the fixed pipeline computes it.",
        &[o("vec4", &[], D_LEGACY, E_NONE)];

    "shadow1D", D_LEGACY, E_NONE, true,
        "Depth comparison lookup in a 1D depth texture. Replaced by `texture` in 1.30.",
        &[o("vec4", &[p("sampler", "sampler1DShadow"), p("coord", "vec3"),
            opt("bias", "float")], D_LEGACY, E_NONE)];
    "shadow1DLod", D_LEGACY, E_NONE, true,
        "`shadow1D` with an explicit level of detail.",
        &[o("vec4", &[p("sampler", "sampler1DShadow"), p("coord", "vec3"),
            p("lod", "float")], D_LEGACY, E_NONE)];
    "shadow1DProj", D_LEGACY, E_NONE, true,
        "`shadow1D` with the coordinate divided by its last component.",
        &[o("vec4", &[p("sampler", "sampler1DShadow"), p("coord", "vec4"),
            opt("bias", "float")], D_LEGACY, E_NONE)];
    "shadow1DProjLod", D_LEGACY, E_NONE, true,
        "`shadow1DProj` with an explicit level of detail.",
        &[o("vec4", &[p("sampler", "sampler1DShadow"), p("coord", "vec4"),
            p("lod", "float")], D_LEGACY, E_NONE)];

    "shadow2D", D_LEGACY, E_NONE, true,
        "Depth comparison lookup in a 2D depth texture. Replaced by `texture` in 1.30.",
        &[o("vec4", &[p("sampler", "sampler2DShadow"), p("coord", "vec3"),
            opt("bias", "float")], D_LEGACY, E_NONE)];
    "shadow2DLod", D_LEGACY, E_NONE, true,
        "`shadow2D` with an explicit level of detail.",
        &[o("vec4", &[p("sampler", "sampler2DShadow"), p("coord", "vec3"),
            p("lod", "float")], D_LEGACY, E_NONE)];
    "shadow2DProj", D_LEGACY, E_NONE, true,
        "`shadow2D` with the coordinate divided by its last component.",
        &[o("vec4", &[p("sampler", "sampler2DShadow"), p("coord", "vec4"),
            opt("bias", "float")], D_LEGACY, E_NONE)];
    "shadow2DProjLod", D_LEGACY, E_NONE, true,
        "`shadow2DProj` with an explicit level of detail.",
        &[o("vec4", &[p("sampler", "sampler2DShadow"), p("coord", "vec4"),
            p("lod", "float")], D_LEGACY, E_NONE)];
    "shadow2DRect", D_LEGACY, E_NONE, true,
        "Depth comparison lookup in a rectangle depth texture, in texel coordinates.",
        &[o("vec4", &[p("sampler", "sampler2DRectShadow"), p("coord", "vec3")],
            D_LEGACY, E_NONE)];
    "shadow2DRectProj", D_LEGACY, E_NONE, true,
        "`shadow2DRect` with the coordinate divided by its last component.",
        &[o("vec4", &[p("sampler", "sampler2DRectShadow"), p("coord", "vec4")],
            D_LEGACY, E_NONE)];

    "texture1D", D_LEGACY, E_NONE, true,
        "Samples a 1D texture. Replaced by the overloaded `texture` in 1.30.",
        &[o("vec4", &[p("sampler", "sampler1D"), p("coord", "float"),
            opt("bias", "float")], D_LEGACY, E_NONE)];
    "texture1DLod", D_LEGACY, E_NONE, true,
        "`texture1D` with an explicit level of detail.",
        &[o("vec4", &[p("sampler", "sampler1D"), p("coord", "float"),
            p("lod", "float")], D_LEGACY, E_NONE)];
    "texture1DProj", D_LEGACY, E_NONE, true,
        "`texture1D` with the coordinate divided by its last component.",
        &[o("vec4", &[p("sampler", "sampler1D"), p("coord", "vec2"),
            opt("bias", "float")], D_LEGACY, E_NONE),
          o("vec4", &[p("sampler", "sampler1D"), p("coord", "vec4"),
            opt("bias", "float")], D_LEGACY, E_NONE)];
    "texture1DProjLod", D_LEGACY, E_NONE, true,
        "`texture1DProj` with an explicit level of detail.",
        &[o("vec4", &[p("sampler", "sampler1D"), p("coord", "vec2"),
            p("lod", "float")], D_LEGACY, E_NONE),
          o("vec4", &[p("sampler", "sampler1D"), p("coord", "vec4"),
            p("lod", "float")], D_LEGACY, E_NONE)];

    "texture2D", D_LEGACY, E_LEGACY, true,
        "Samples a 2D texture. The WebGL1 and OpenGL ES 1.00 spelling; replaced by the \
         overloaded `texture` in desktop 1.30 and ES 3.00.",
        &[o("vec4", &[p("sampler", "sampler2D"), p("coord", "vec2"),
            opt("bias", "float")], D_LEGACY, E_LEGACY)];
    "texture2DLod", D_LEGACY, E_LEGACY, true,
        "`texture2D` with an explicit level of detail. Vertex stage only in ES 1.00.",
        &[o("vec4", &[p("sampler", "sampler2D"), p("coord", "vec2"),
            p("lod", "float")], D_LEGACY, E_LEGACY)];
    "texture2DProj", D_LEGACY, E_LEGACY, true,
        "`texture2D` with the coordinate divided by its last component.",
        &[o("vec4", &[p("sampler", "sampler2D"), p("coord", "vec3"),
            opt("bias", "float")], D_LEGACY, E_LEGACY),
          o("vec4", &[p("sampler", "sampler2D"), p("coord", "vec4"),
            opt("bias", "float")], D_LEGACY, E_LEGACY)];
    "texture2DProjLod", D_LEGACY, E_LEGACY, true,
        "`texture2DProj` with an explicit level of detail.",
        &[o("vec4", &[p("sampler", "sampler2D"), p("coord", "vec3"),
            p("lod", "float")], D_LEGACY, E_LEGACY),
          o("vec4", &[p("sampler", "sampler2D"), p("coord", "vec4"),
            p("lod", "float")], D_LEGACY, E_LEGACY)];
    "texture2DRect", D_LEGACY, E_NONE, true,
        "Samples a rectangle texture, in unnormalised texel coordinates.",
        &[o("vec4", &[p("sampler", "sampler2DRect"), p("coord", "vec2")],
            D_LEGACY, E_NONE)];
    "texture2DRectProj", D_LEGACY, E_NONE, true,
        "`texture2DRect` with the coordinate divided by its last component.",
        &[o("vec4", &[p("sampler", "sampler2DRect"), p("coord", "vec3")],
            D_LEGACY, E_NONE),
          o("vec4", &[p("sampler", "sampler2DRect"), p("coord", "vec4")],
            D_LEGACY, E_NONE)];

    "texture3D", D_LEGACY, E_NONE, true,
        "Samples a 3D texture. Replaced by the overloaded `texture` in 1.30.",
        &[o("vec4", &[p("sampler", "sampler3D"), p("coord", "vec3"),
            opt("bias", "float")], D_LEGACY, E_NONE)];
    "texture3DLod", D_LEGACY, E_NONE, true,
        "`texture3D` with an explicit level of detail.",
        &[o("vec4", &[p("sampler", "sampler3D"), p("coord", "vec3"),
            p("lod", "float")], D_LEGACY, E_NONE)];
    "texture3DProj", D_LEGACY, E_NONE, true,
        "`texture3D` with the coordinate divided by its last component.",
        &[o("vec4", &[p("sampler", "sampler3D"), p("coord", "vec4"),
            opt("bias", "float")], D_LEGACY, E_NONE)];
    "texture3DProjLod", D_LEGACY, E_NONE, true,
        "`texture3DProj` with an explicit level of detail.",
        &[o("vec4", &[p("sampler", "sampler3D"), p("coord", "vec4"),
            p("lod", "float")], D_LEGACY, E_NONE)];

    "textureCube", D_LEGACY, E_LEGACY, true,
        "Samples a cube map with a direction vector. Replaced by `texture` in desktop \
         1.30 and ES 3.00.",
        &[o("vec4", &[p("sampler", "samplerCube"), p("coord", "vec3"),
            opt("bias", "float")], D_LEGACY, E_LEGACY)];
    "textureCubeLod", D_LEGACY, E_LEGACY, true,
        "`textureCube` with an explicit level of detail. Vertex stage only in ES 1.00.",
        &[o("vec4", &[p("sampler", "samplerCube"), p("coord", "vec3"),
            p("lod", "float")], D_LEGACY, E_LEGACY)];
}

// -- variables and constants ----------------------------------------------
//
// The legacy half is GLSL 1.20 §7.1–7.6 and GLSL ES 1.00 §7. The constants are
// 4.60 §7.3 and are *not* legacy — they are current in every version and simply
// have no reference page, so their masks are permissive on purpose: refusing a
// legal name is the one failure mode this crate must not have.

legacy_variables! {
    "gl_BackColor", "vec4", VERTEX_GEOMETRY, Out, D_LEGACY, E_NONE, true,
        "The back-face primary colour a vertex shader writes for the fixed pipeline.";
    "gl_BackSecondaryColor", "vec4", VERTEX_GEOMETRY, Out, D_LEGACY, E_NONE, true,
        "The back-face secondary colour a vertex shader writes.";
    "gl_ClipVertex", "vec4", VERTEX_GEOMETRY, Out, D_LEGACY, E_NONE, true,
        "The vertex position user clip planes are applied to. Replaced by \
         `gl_ClipDistance`.";
    "gl_Color", "vec4", VARYING, InOut, D_LEGACY, E_NONE, true,
        "The primary colour: a vertex attribute in the vertex stage, the interpolated \
         value in the fragment stage.";
    "gl_DepthRange", "gl_DepthRangeParameters", StageMask::ALL, In, D_ALL, E_ALL, false,
        "The depth range of the viewport, as `near`, `far` and `diff`.";
    "gl_FogCoord", "float", VERTEX, In, D_LEGACY, E_NONE, true,
        "The fog coordinate vertex attribute.";
    "gl_FogFragCoord", "float", VARYING, InOut, D_LEGACY, E_NONE, true,
        "The fog coordinate a vertex shader writes and a fragment shader reads.";
    "gl_FragColor", "vec4", FRAGMENT, Out, D_LEGACY, E_LEGACY, true,
        "The colour a fragment shader writes. Replaced by a user-declared `out` \
         variable in desktop 1.30 and ES 3.00.";
    "gl_FragData", "vec4[]", FRAGMENT, Out, D_LEGACY, E_LEGACY, true,
        "One colour per draw buffer, `gl_FragData[gl_MaxDrawBuffers]`. Replaced by \
         user-declared `out` variables.";
    "gl_FrontColor", "vec4", VERTEX_GEOMETRY, Out, D_LEGACY, E_NONE, true,
        "The front-face primary colour a vertex shader writes.";
    "gl_FrontSecondaryColor", "vec4", VERTEX_GEOMETRY, Out, D_LEGACY, E_NONE, true,
        "The front-face secondary colour a vertex shader writes.";
    "gl_LightSource", "gl_LightSourceParameters[]", StageMask::ALL, In, D_LEGACY, E_NONE,
        true, "The fixed-function light state, one entry per light.";
    "gl_ModelViewMatrix", "mat4", StageMask::ALL, In, D_LEGACY, E_NONE, true,
        "The fixed-function model-view matrix.";
    "gl_ModelViewMatrixInverse", "mat4", StageMask::ALL, In, D_LEGACY, E_NONE, true,
        "The inverse of `gl_ModelViewMatrix`.";
    "gl_ModelViewMatrixTranspose", "mat4", StageMask::ALL, In, D_LEGACY, E_NONE, true,
        "The transpose of `gl_ModelViewMatrix`.";
    "gl_ModelViewProjectionMatrix", "mat4", StageMask::ALL, In, D_LEGACY, E_NONE, true,
        "`gl_ProjectionMatrix * gl_ModelViewMatrix`.";
    "gl_ModelViewProjectionMatrixInverse", "mat4", StageMask::ALL, In, D_LEGACY, E_NONE,
        true, "The inverse of `gl_ModelViewProjectionMatrix`.";
    "gl_MultiTexCoord0", "vec4", VERTEX, In, D_LEGACY, E_NONE, true,
        "Texture coordinate set 0, as a vertex attribute.";
    "gl_MultiTexCoord1", "vec4", VERTEX, In, D_LEGACY, E_NONE, true,
        "Texture coordinate set 1, as a vertex attribute.";
    "gl_MultiTexCoord2", "vec4", VERTEX, In, D_LEGACY, E_NONE, true,
        "Texture coordinate set 2, as a vertex attribute.";
    "gl_MultiTexCoord3", "vec4", VERTEX, In, D_LEGACY, E_NONE, true,
        "Texture coordinate set 3, as a vertex attribute.";
    "gl_Normal", "vec3", VERTEX, In, D_LEGACY, E_NONE, true,
        "The normal vertex attribute of the fixed pipeline.";
    "gl_NormalMatrix", "mat3", StageMask::ALL, In, D_LEGACY, E_NONE, true,
        "The inverse transpose of the upper 3×3 of `gl_ModelViewMatrix`.";
    "gl_NormalScale", "float", StageMask::ALL, In, D_LEGACY, E_NONE, true,
        "The fixed-function normal rescaling factor.";
    "gl_ProjectionMatrix", "mat4", StageMask::ALL, In, D_LEGACY, E_NONE, true,
        "The fixed-function projection matrix.";
    "gl_ProjectionMatrixInverse", "mat4", StageMask::ALL, In, D_LEGACY, E_NONE, true,
        "The inverse of `gl_ProjectionMatrix`.";
    "gl_SecondaryColor", "vec4", VARYING, InOut, D_LEGACY, E_NONE, true,
        "The secondary colour: a vertex attribute, then the interpolated value.";
    "gl_TexCoord", "vec4[]", VARYING, InOut, D_LEGACY, E_NONE, true,
        "The interpolated texture coordinate sets, \
         `gl_TexCoord[gl_MaxTextureCoords]`.";
    "gl_TextureMatrix", "mat4[]", StageMask::ALL, In, D_LEGACY, E_NONE, true,
        "The fixed-function texture matrices, one per coordinate set.";
    "gl_Vertex", "vec4", VERTEX, In, D_LEGACY, E_NONE, true,
        "The position vertex attribute of the fixed pipeline.";

    // -- builtin constants, 4.60 §7.3 and ES 3.20 §7.4 ---------------------
    "gl_MaxAtomicCounterBindings", "int", StageMask::ALL, In, D_ALL, E_ALL, false,
        "Implementation limit: atomic counter binding points.";
    "gl_MaxClipDistances", "int", StageMask::ALL, In, D_ALL, E_ALL, false,
        "Implementation limit: entries in `gl_ClipDistance`.";
    "gl_MaxClipPlanes", "int", StageMask::ALL, In, D_LEGACY, E_NONE, true,
        "Implementation limit: fixed-function clip planes.";
    "gl_MaxCombinedTextureImageUnits", "int", StageMask::ALL, In, D_ALL, E_ALL, false,
        "Implementation limit: texture image units across all stages.";
    "gl_MaxComputeWorkGroupCount", "ivec3", StageMask::ALL, In, D_ALL, E_ALL, false,
        "Implementation limit: work groups per dispatch.";
    "gl_MaxComputeWorkGroupSize", "ivec3", StageMask::ALL, In, D_ALL, E_ALL, false,
        "Implementation limit: invocations per work group, per dimension.";
    "gl_MaxDrawBuffers", "int", StageMask::ALL, In, D_ALL, E_ALL, false,
        "Implementation limit: colour attachments a fragment shader may write.";
    "gl_MaxFragmentInputComponents", "int", StageMask::ALL, In, D_ALL, E_ALL, false,
        "Implementation limit: fragment shader input components.";
    "gl_MaxFragmentUniformComponents", "int", StageMask::ALL, In, D_ALL, E_ALL, false,
        "Implementation limit: fragment shader uniform components.";
    "gl_MaxFragmentUniformVectors", "int", StageMask::ALL, In, D_ALL, E_ALL, false,
        "Implementation limit: fragment shader uniform vectors.";
    "gl_MaxGeometryOutputVertices", "int", StageMask::ALL, In, D_ALL, E_ALL, false,
        "Implementation limit: vertices one geometry shader invocation may emit.";
    "gl_MaxPatchVertices", "int", StageMask::ALL, In, D_ALL, E_ALL, false,
        "Implementation limit: vertices in an input patch.";
    "gl_MaxProgramTexelOffset", "int", StageMask::ALL, In, D_ALL, E_ALL, false,
        "Implementation limit: the largest texel offset a lookup may take.";
    "gl_MinProgramTexelOffset", "int", StageMask::ALL, In, D_ALL, E_ALL, false,
        "Implementation limit: the smallest texel offset a lookup may take.";
    "gl_MaxSamples", "int", StageMask::ALL, In, D_ALL, E_ALL, false,
        "Implementation limit: samples in a multisample surface.";
    "gl_MaxTessGenLevel", "int", StageMask::ALL, In, D_ALL, E_ALL, false,
        "Implementation limit: tessellation level.";
    "gl_MaxTextureCoords", "int", StageMask::ALL, In, D_LEGACY, E_NONE, true,
        "Implementation limit: entries in `gl_TexCoord`.";
    "gl_MaxTextureImageUnits", "int", StageMask::ALL, In, D_ALL, E_ALL, false,
        "Implementation limit: texture image units a fragment shader may use.";
    "gl_MaxTextureUnits", "int", StageMask::ALL, In, D_LEGACY, E_NONE, true,
        "Implementation limit: fixed-function texture units.";
    "gl_MaxVaryingComponents", "int", StageMask::ALL, In, D_ALL, E_NONE, false,
        "Implementation limit: components passed between stages.";
    "gl_MaxVaryingFloats", "int", StageMask::ALL, In, D_LEGACY, E_NONE, true,
        "Implementation limit: floats passed between stages. Pre-1.30 spelling.";
    "gl_MaxVaryingVectors", "int", StageMask::ALL, In, D_ALL, E_ALL, false,
        "Implementation limit: vectors passed between stages.";
    "gl_MaxVertexAttribs", "int", StageMask::ALL, In, D_ALL, E_ALL, false,
        "Implementation limit: vertex attributes.";
    "gl_MaxVertexOutputComponents", "int", StageMask::ALL, In, D_ALL, E_ALL, false,
        "Implementation limit: vertex shader output components.";
    "gl_MaxVertexTextureImageUnits", "int", StageMask::ALL, In, D_ALL, E_ALL, false,
        "Implementation limit: texture image units a vertex shader may use.";
    "gl_MaxVertexUniformComponents", "int", StageMask::ALL, In, D_ALL, E_ALL, false,
        "Implementation limit: vertex shader uniform components.";
    "gl_MaxVertexUniformVectors", "int", StageMask::ALL, In, D_ALL, E_ALL, false,
        "Implementation limit: vertex shader uniform vectors.";
    "gl_MaxViewports", "int", StageMask::ALL, In, D_ALL, E_NONE, false,
        "Implementation limit: viewports.";
}

/// The legacy or compatibility function of that name, if the table has one.
pub fn legacy_function(name: &str) -> Option<&'static LegacyFunction> {
    LEGACY_FUNCTIONS.iter().find(|f| f.function.name == name)
}

/// The legacy variable or builtin constant of that name, if the table has one.
pub fn legacy_variable(name: &str) -> Option<&'static LegacyVariable> {
    LEGACY_VARIABLES.iter().find(|v| v.variable.name == name)
}
