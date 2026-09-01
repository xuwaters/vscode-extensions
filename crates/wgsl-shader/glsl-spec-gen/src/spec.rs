//! What the generator knows that docs.gl does not.
//!
//! Two closed sets, both from the OpenGL Shading Language specification rather
//! than the reference pages: how a generic family expands, and how docs.gl
//! misspells a handful of type names. Both are small, both are cited, and
//! neither belongs in `glsl-spec` — the crate that ships should carry the
//! *answer*, not the reasoning that produced it.

/// docs.gl's type-name errata, applied to every type token the generator reads.
///
/// Every entry is evidenced in `research/docs-gl.md` §3.1. The list is closed:
/// a type token that is neither a known concrete type nor a family after this
/// substitution stops the run.
const FIXUPS: &[(&str, &str)] = &[
    // `textureQueryLevels`, `textureQueryLod`: a doubled `D`.
    ("gsampler2DDArray", "gsampler2DArray"),
    // `textureSize` alone spells the rectangle families without the `2D`,
    // while `texture` and the rest spell them with it. Its version-table row
    // drops the `g` on top of that, hence all four spellings.
    ("gsamplerRect", "gsampler2DRect"),
    ("gsamplerRectShadow", "gsampler2DRectShadow"),
    ("samplerRect", "sampler2DRect"),
    ("samplerRectShadow", "sampler2DRectShadow"),
    // The image pages invert the same two names.
    ("gbufferImage", "gimageBuffer"),
    ("gimageRect", "gimage2DRect"),
];

/// The spelling to use for a type token as docs.gl wrote it.
pub fn normalize_type(raw: &str) -> &str {
    for (from, to) in FIXUPS {
        if raw == *from {
            return to;
        }
    }
    raw
}

/// The scalar/vector/matrix generic families, spelled as the spec spells them.
const SCALAR_FAMILIES: &[(&str, &[&str])] = &[
    ("genType", &["float", "vec2", "vec3", "vec4"]),
    ("genIType", &["int", "ivec2", "ivec3", "ivec4"]),
    ("genUType", &["uint", "uvec2", "uvec3", "uvec4"]),
    ("genBType", &["bool", "bvec2", "bvec3", "bvec4"]),
    ("genDType", &["double", "dvec2", "dvec3", "dvec4"]),
    ("vec", &["vec2", "vec3", "vec4"]),
    ("ivec", &["ivec2", "ivec3", "ivec4"]),
    ("uvec", &["uvec2", "uvec3", "uvec4"]),
    ("bvec", &["bvec2", "bvec3", "bvec4"]),
    ("dvec", &["dvec2", "dvec3", "dvec4"]),
    (
        "mat",
        &[
            "mat2", "mat3", "mat4", "mat2x2", "mat2x3", "mat2x4", "mat3x2", "mat3x3",
            "mat3x4", "mat4x2", "mat4x3", "mat4x4",
        ],
    ),
    (
        "dmat",
        &[
            "dmat2", "dmat3", "dmat4", "dmat2x2", "dmat2x3", "dmat2x4", "dmat3x2",
            "dmat3x3", "dmat3x4", "dmat4x2", "dmat4x3", "dmat4x4",
        ],
    ),
];

/// What a generic family expands to, or `None` when the name is a concrete
/// type.
///
/// The `g`-prefixed opaque families follow one rule rather than a table:
/// `gsampler2D` is the float, int and unsigned flavours of `sampler2D`. The
/// exception is the shadow samplers — GLSL has `sampler2DShadow` and no
/// `isampler2DShadow`, so `gsampler2DArrayShadow` (which is how the reference
/// pages write it, loosely) expands to the single float flavour.
pub fn family_members(name: &str) -> Option<Vec<String>> {
    for (family, members) in SCALAR_FAMILIES {
        if name == *family {
            return Some(members.iter().map(|m| m.to_string()).collect());
        }
    }
    if let Some(base) = name.strip_prefix('g') {
        let opaque = base.starts_with("sampler") || base.starts_with("image");
        let vector = base.starts_with("vec") && base.len() == 4;
        if opaque || vector {
            if base.ends_with("Shadow") {
                return Some(vec![base.to_string()]);
            }
            return Some(vec![
                base.to_string(),
                format!("i{base}"),
                format!("u{base}"),
            ]);
        }
    }
    None
}

/// Whether the token names a family at all.
pub fn is_family(name: &str) -> bool {
    family_members(name).is_some()
}

/// The spelling to compare a version-table row's qualifier against a
/// prototype's type with.
///
/// The tables and the prototypes disagree about the leading `g` on opaque
/// families — `textureSize`'s row says `samplerBuffer` where its prototype says
/// `gsamplerBuffer` — and about nothing else, so the key drops it.
pub fn comparison_key(ty: &str) -> String {
    let ty = normalize_type(ty);
    match ty.strip_prefix('g') {
        Some(base) if base.starts_with("sampler") || base.starts_with("image") => {
            base.to_string()
        }
        _ => ty.to_string(),
    }
}
