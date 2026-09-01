//! The closed sets docs.gl does not document: keywords, basic types, and the
//! precision defaults GLSL ES predeclares.
//!
//! Hand-written from the specification text per
//! [decision 0002](../../../../docs/rfc/012-glsl-analyzer/decisions/0002-docs-gl-as-spec-source.md)
//! — the reference pages cover builtin *functions and variables* and nothing
//! else, so scraping these would mean scraping the spec HTML, which is a second
//! fragile pipeline for a few hundred stable words. Sources: OpenGL Shading
//! Language 4.60 §3.6 (keywords), §4.1 (basic types), §4.7 (precision); GLSL ES
//! 3.00 §4.5.4 and 3.10 §4.7.4 for the predeclared precision statements.
//!
//! Type names are keywords too, but they live in [`BASIC_TYPES`] rather than
//! being repeated in [`KEYWORDS`]; [`is_keyword`] asks both. The tables stay
//! grouped by arrival rather than sorted alphabetically, because that is the
//! order the spec lists them in and the only order they can be checked against
//! it in. Lookup does not scan them: [`KEYWORD_ORDER`] and [`TYPE_ORDER`] are
//! alphabetical index arrays, sorted at compile time, and every lookup here
//! binary-searches one of them. That matters — every identifier in a file is
//! classified against both tables, and the linear scan the two once used was
//! the single largest cost in the whole pipeline (RFC 012 P5-10).

use crate::model::Stage;
use crate::version::{DesktopMask, DesktopVersion, EsMask, EsVersion, Version};

/// What a keyword is for. Coarse on purpose: fine enough to colour and to
/// group a completion list, not a grammar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeywordKind {
    /// `const`, `uniform`, `in`, `buffer` — where the storage lives.
    Storage,
    /// `flat`, `smooth`, `centroid`, `invariant`, `precise`, `layout`.
    Qualifier,
    /// `if`, `for`, `return`, `discard`.
    Control,
    /// `lowp`, `mediump`, `highp`, `precision`.
    Precision,
    /// `struct`, `true`, `false`, `subroutine`.
    Other,
}

/// A reserved word of the language.
#[derive(Debug, Clone, Copy)]
pub struct Keyword {
    pub word: &'static str,
    pub kind: KeywordKind,
    /// Versions of the desktop **core** profile that have it.
    pub desktop: DesktopMask,
    pub es: EsMask,
    /// Whether the compatibility profile keeps it past the core mask —
    /// `attribute` and `varying` are core only through 1.30 but legal forever
    /// in `#version N compatibility`.
    pub compatibility: bool,
}

/// What kind of thing a basic type is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeKind {
    Void,
    /// `bool int uint float double`.
    Scalar,
    /// `vec3 ivec2 bvec4 dvec4`.
    Vector,
    /// `mat4 mat2x3 dmat4x2`.
    Matrix,
    /// `sampler2D isamplerCube sampler2DArrayShadow`.
    Sampler,
    /// `image3D uimageBuffer`.
    Image,
    /// `atomic_uint`.
    AtomicCounter,
}

/// A predeclared type name.
#[derive(Debug, Clone, Copy)]
pub struct BasicType {
    pub name: &'static str,
    pub kind: TypeKind,
    pub desktop: DesktopMask,
    pub es: EsMask,
}

/// The precision a GLSL ES shader gets when it declares none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrecisionDefault {
    Low,
    Medium,
    High,
    /// No default exists — the shader must declare one before using the type.
    /// `float` in an ES fragment shader, and nothing else.
    Required,
}

macro_rules! keyword_table {
    (@flag) => { false };
    (@flag $marker:ident) => { true };
    ($($word:literal, $kind:ident, $desktop:expr, $es:expr $(, $compat:ident)?;)+) => {
        /// Every reserved word that is not a type name. See [`BASIC_TYPES`] for
        /// those, and [`is_keyword`] for the union.
        pub const KEYWORDS: &[Keyword] = &[$(Keyword {
            word: $word,
            kind: KeywordKind::$kind,
            desktop: $desktop,
            es: $es,
            compatibility: keyword_table!(@flag $($compat)?),
        }),+];
    };
}

macro_rules! type_table {
    ($($kind:ident, $desktop:expr, $es:expr => $($name:literal),+ ;)+) => {
        /// Every predeclared type name, grouped by when it arrived.
        pub const BASIC_TYPES: &[BasicType] = &[$($(BasicType {
            name: $name,
            kind: TypeKind::$kind,
            desktop: $desktop,
            es: $es,
        }),+),+];
    };
}

const D110: DesktopMask = DesktopMask::since(DesktopVersion::V110);
const D120: DesktopMask = DesktopMask::since(DesktopVersion::V120);
const D130: DesktopMask = DesktopMask::since(DesktopVersion::V130);
const D140: DesktopMask = DesktopMask::since(DesktopVersion::V140);
const D150: DesktopMask = DesktopMask::since(DesktopVersion::V150);
const D400: DesktopMask = DesktopMask::since(DesktopVersion::V400);
const D420: DesktopMask = DesktopMask::since(DesktopVersion::V420);
const D430: DesktopMask = DesktopMask::since(DesktopVersion::V430);
/// Core only up to and including 1.30, then deprecated out — `attribute` and
/// `varying`. The compatibility profile keeps them; that is the `compat` flag.
const D_LEGACY: DesktopMask = DesktopMask::through(DesktopVersion::V110, DesktopVersion::V130);

const E100: EsMask = EsMask::since(EsVersion::V100);
const E300: EsMask = EsMask::since(EsVersion::V300);
const E310: EsMask = EsMask::since(EsVersion::V310);
const E320: EsMask = EsMask::since(EsVersion::V320);
const E_LEGACY: EsMask = EsMask::only(EsVersion::V100);
const ENONE: EsMask = EsMask::EMPTY;

keyword_table! {
    // ── Storage ───────────────────────────────────────────────────────────
    "const",        Storage,    D110, E100;
    "uniform",      Storage,    D110, E100;
    "in",           Storage,    D110, E100;
    "out",          Storage,    D110, E100;
    "inout",        Storage,    D110, E100;
    "buffer",       Storage,    D430, E310;
    "shared",       Storage,    D430, E310;
    "attribute",    Storage,    D_LEGACY, E_LEGACY, compat;
    "varying",      Storage,    D_LEGACY, E_LEGACY, compat;
    "subroutine",   Storage,    D400, ENONE;

    // ── Qualifiers ────────────────────────────────────────────────────────
    "layout",       Qualifier,  D140, E300;
    "centroid",     Qualifier,  D120, E300;
    "sample",       Qualifier,  D400, E320;
    "patch",        Qualifier,  D400, E320;
    "invariant",    Qualifier,  D120, E100;
    "precise",      Qualifier,  D400, E320;
    "flat",         Qualifier,  D130, E300;
    "smooth",       Qualifier,  D130, E300;
    "noperspective", Qualifier, D130, ENONE;
    "coherent",     Qualifier,  D420, E310;
    "volatile",     Qualifier,  D420, E310;
    "restrict",     Qualifier,  D420, E310;
    "readonly",     Qualifier,  D420, E310;
    "writeonly",    Qualifier,  D420, E310;

    // ── Control flow ──────────────────────────────────────────────────────
    "if",           Control,    D110, E100;
    "else",         Control,    D110, E100;
    "for",          Control,    D110, E100;
    "while",        Control,    D110, E100;
    "do",           Control,    D110, E100;
    "break",        Control,    D110, E100;
    "continue",     Control,    D110, E100;
    "return",       Control,    D110, E100;
    "discard",      Control,    D110, E100;
    "switch",       Control,    D130, E300;
    "case",         Control,    D130, E300;
    "default",      Control,    D130, E300;

    // ── Precision ─────────────────────────────────────────────────────────
    // Desktop accepts these from 1.30 and gives them no semantic meaning
    // (4.60 §4.7.4); in ES they are load-bearing.
    "precision",    Precision,  D130, E100;
    "lowp",         Precision,  D130, E100;
    "mediump",      Precision,  D130, E100;
    "highp",        Precision,  D130, E100;

    // ── Everything else ───────────────────────────────────────────────────
    "struct",       Other,      D110, E100;
    "true",         Other,      D110, E100;
    "false",        Other,      D110, E100;
}

type_table! {
    Void,   D110, E100 => "void";
    Scalar, D110, E100 => "bool", "int", "float";
    Scalar, D130, E300 => "uint";
    Scalar, D400, ENONE => "double";

    Vector, D110, E100 => "vec2", "vec3", "vec4",
                          "ivec2", "ivec3", "ivec4",
                          "bvec2", "bvec3", "bvec4";
    Vector, D130, E300 => "uvec2", "uvec3", "uvec4";
    Vector, D400, ENONE => "dvec2", "dvec3", "dvec4";

    Matrix, D110, E100 => "mat2", "mat3", "mat4";
    Matrix, D120, E300 => "mat2x2", "mat2x3", "mat2x4",
                          "mat3x2", "mat3x3", "mat3x4",
                          "mat4x2", "mat4x3", "mat4x4";
    Matrix, D400, ENONE => "dmat2", "dmat3", "dmat4",
                           "dmat2x2", "dmat2x3", "dmat2x4",
                           "dmat3x2", "dmat3x3", "dmat3x4",
                           "dmat4x2", "dmat4x3", "dmat4x4";

    // ES 1.00 predeclares only these two samplers; everything else is ES 3.00
    // at the earliest.
    Sampler, D110, E100 => "sampler2D", "samplerCube";
    Sampler, D110, ENONE => "sampler1D", "sampler1DShadow", "sampler2DShadow";
    Sampler, D130, E300 => "sampler3D", "samplerCubeShadow",
                           "sampler2DArray", "sampler2DArrayShadow",
                           "isampler2D", "isampler3D", "isamplerCube", "isampler2DArray",
                           "usampler2D", "usampler3D", "usamplerCube", "usampler2DArray";
    Sampler, D130, ENONE => "sampler1DArray", "sampler1DArrayShadow",
                            "isampler1D", "isampler1DArray",
                            "usampler1D", "usampler1DArray";
    Sampler, D140, ENONE => "sampler2DRect", "sampler2DRectShadow",
                            "isampler2DRect", "usampler2DRect";
    Sampler, D140, E320 => "samplerBuffer", "isamplerBuffer", "usamplerBuffer";
    Sampler, D150, E310 => "sampler2DMS", "isampler2DMS", "usampler2DMS";
    Sampler, D150, E320 => "sampler2DMSArray", "isampler2DMSArray", "usampler2DMSArray";
    Sampler, D400, E320 => "samplerCubeArray", "samplerCubeArrayShadow",
                           "isamplerCubeArray", "usamplerCubeArray";

    Image, D420, E310 => "image2D", "iimage2D", "uimage2D",
                         "image3D", "iimage3D", "uimage3D",
                         "imageCube", "iimageCube", "uimageCube",
                         "image2DArray", "iimage2DArray", "uimage2DArray";
    Image, D420, E320 => "imageBuffer", "iimageBuffer", "uimageBuffer",
                         "imageCubeArray", "iimageCubeArray", "uimageCubeArray";
    Image, D420, ENONE => "image1D", "iimage1D", "uimage1D",
                          "image1DArray", "iimage1DArray", "uimage1DArray",
                          "image2DRect", "iimage2DRect", "uimage2DRect",
                          "image2DMS", "iimage2DMS", "uimage2DMS",
                          "image2DMSArray", "iimage2DMSArray", "uimage2DMSArray";

    AtomicCounter, D420, E310 => "atomic_uint";
}

/// Words the spec reserves for future use. Using one is an error, and saying
/// so plainly is far kinder than "syntax error". GLSL 4.60 §3.6.
pub const RESERVED_KEYWORDS: &[&str] = &[
    "active", "asm", "cast", "class", "common", "enum", "extern", "external", "filter",
    "fixed", "fvec2", "fvec3", "fvec4", "goto", "half", "hvec2", "hvec3", "hvec4",
    "inline", "input", "interface", "long", "namespace", "noinline", "output",
    "partition", "public", "resource", "sampler3DRect", "short", "sizeof", "static",
    "superp", "template", "this", "typedef", "union", "unsigned", "using",
];

/// Whether `a` sorts before `b`, in a `const` context.
///
/// `str`'s own `Ord` is not `const`, and these orders are built at compile time
/// so that a lookup costs no initialisation and no runtime state.
const fn before(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let shortest = if a.len() < b.len() { a.len() } else { b.len() };
    let mut i = 0;
    while i < shortest {
        if a[i] != b[i] {
            return a[i] < b[i];
        }
        i += 1;
    }
    a.len() < b.len()
}

/// Insertion-sort `order` by the word each index names. Insertion sort because
/// it is six lines of `const`-compatible Rust and these tables are hundreds of
/// entries, not millions.
macro_rules! sorted_order {
    ($table:expr, $word:ident) => {{
        let mut order = [0u16; $table.len()];
        let mut i = 0;
        while i < order.len() {
            order[i] = i as u16;
            i += 1;
        }
        let mut i = 1;
        while i < order.len() {
            let mut j = i;
            while j > 0
                && before($table[order[j] as usize].$word, $table[order[j - 1] as usize].$word)
            {
                let swap = order[j];
                order[j] = order[j - 1];
                order[j - 1] = swap;
                j -= 1;
            }
            i += 1;
        }
        order
    }};
}

/// Where each first byte's run begins in an alphabetical order: the entries
/// starting with byte `b` are `index[b] .. index[b + 1]`.
///
/// This is what makes the common answer *no* cost one array read. Nearly every
/// identifier in a shader starts with a letter no keyword or type does —
/// `helper`, `normal`, `lambert`, `tinted` — and those never reach the search
/// at all.
macro_rules! first_byte_index {
    ($table:expr, $order:expr, $word:ident) => {{
        let mut index = [0u16; 257];
        let mut i = 0;
        while i < $order.len() {
            let byte = $table[$order[i] as usize].$word.as_bytes()[0];
            index[byte as usize + 1] += 1;
            i += 1;
        }
        let mut byte = 1;
        while byte < 257 {
            index[byte] += index[byte - 1];
            byte += 1;
        }
        index
    }};
}

/// [`KEYWORDS`] in alphabetical order, as indices.
pub const KEYWORD_ORDER: [u16; KEYWORDS.len()] = sorted_order!(KEYWORDS, word);

/// [`BASIC_TYPES`] in alphabetical order, as indices.
pub const TYPE_ORDER: [u16; BASIC_TYPES.len()] = sorted_order!(BASIC_TYPES, name);

const KEYWORD_FIRST: [u16; 257] = first_byte_index!(KEYWORDS, KEYWORD_ORDER, word);
const TYPE_FIRST: [u16; 257] = first_byte_index!(BASIC_TYPES, TYPE_ORDER, name);

/// The index in `order` whose name is `wanted` — first byte, then binary
/// search inside that byte's run.
fn find(
    order: &[u16],
    first: &[u16; 257],
    wanted: &str,
    name: impl Fn(usize) -> &'static str,
) -> Option<usize> {
    let byte = *wanted.as_bytes().first()? as usize;
    let mut low = first[byte] as usize;
    let mut high = first[byte + 1] as usize;
    while low < high {
        let mid = (low + high) / 2;
        let entry = order[mid] as usize;
        match name(entry).cmp(wanted) {
            std::cmp::Ordering::Less => low = mid + 1,
            std::cmp::Ordering::Greater => high = mid,
            std::cmp::Ordering::Equal => return Some(entry),
        }
    }
    None
}

/// The keyword of that name, type names excluded.
pub fn keyword(word: &str) -> Option<&'static Keyword> {
    find(&KEYWORD_ORDER, &KEYWORD_FIRST, word, |i| KEYWORDS[i].word).map(|i| &KEYWORDS[i])
}

/// The predeclared type of that name.
pub fn basic_type(name: &str) -> Option<&'static BasicType> {
    find(&TYPE_ORDER, &TYPE_FIRST, name, |i| BASIC_TYPES[i].name).map(|i| &BASIC_TYPES[i])
}

/// Whether the word is reserved by the language — a keyword or a type name.
/// Version-agnostic: a word that is a keyword in *any* version cannot be used
/// as an identifier, which is what a lexer wants to know.
pub fn is_keyword(word: &str) -> bool {
    keyword(word).is_some() || basic_type(word).is_some()
}

/// Whether the word is reserved for future use — legal nowhere.
pub fn is_reserved(word: &str) -> bool {
    RESERVED_KEYWORDS.binary_search(&word).is_ok()
}

/// The precision a type gets in an ES shader that never said. `None` where the
/// language predeclares nothing — every desktop version (where precision
/// qualifiers are accepted and meaningless, 4.60 §4.7.4) and every ES type
/// outside the four the spec names.
///
/// GLSL ES 3.00 §4.5.4 and 3.10 §4.7.4: the vertex and compute languages
/// predeclare `highp float`, `highp int`, `lowp sampler2D`, `lowp samplerCube`;
/// the fragment language predeclares `mediump int` and the two samplers and
/// *no* float, which is why every ES fragment shader in the wild opens with
/// `precision mediump float;`.
pub fn default_precision(version: Version, stage: Stage, ty: &str) -> Option<PrecisionDefault> {
    if !version.is_es() {
        return None;
    }
    let fragment = stage == Stage::Fragment;
    match ty {
        "float" if fragment => Some(PrecisionDefault::Required),
        "float" => Some(PrecisionDefault::High),
        "int" | "uint" if fragment => Some(PrecisionDefault::Medium),
        "int" | "uint" => Some(PrecisionDefault::High),
        "sampler2D" | "samplerCube" => Some(PrecisionDefault::Low),
        _ => None,
    }
}
