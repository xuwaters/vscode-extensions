//! GLSL's predeclared surface as embedded data.
//!
//! RFC 012 (docs/rfc/012-glsl-analyzer). The generated half is produced by
//! `glsl-spec-gen` from a docs.gl checkout; the hand-written half covers the
//! small closed sets — keywords, basic types, precision defaults — the
//! reference pages do not carry.
//!
//! Two questions this crate answers and the old hand-curated tables could not:
//! *which* signature, and *where* it exists.
//!
//! ```
//! use glsl_spec::{DesktopVersion, EsVersion, Version};
//!
//! let mix = glsl_spec::function("mix").unwrap();
//! // Every overload the reference page declares, not just the first.
//! assert!(mix.overloads.len() > 1);
//! // `mix(genDType, genDType, genDType)` is a 4.00 arrival, so 1.30 has fewer.
//! let old = mix.overloads_in(Version::Desktop(DesktopVersion::V130)).count();
//! let new = mix.overloads_in(Version::Desktop(DesktopVersion::V450)).count();
//! assert!(old < new);
//!
//! // And ES is tracked separately, not guessed from the desktop mask.
//! let texture = glsl_spec::function("texture").unwrap();
//! assert!(texture.available_in(Version::Es(EsVersion::V300)));
//! assert!(!texture.available_in(Version::Es(EsVersion::V100)));
//! ```

mod generated;
mod keywords;
mod legacy;
mod model;
mod version;

#[cfg(test)]
mod tests;

pub use generated::{COUNTS, DOCS, DOCS_GL_COMMIT, FAMILIES, FUNCTIONS, GENERATOR, VARIABLES};
pub use keywords::{
    BASIC_TYPES, BasicType, KEYWORD_ORDER, KEYWORDS, Keyword, KeywordKind,
    PrecisionDefault, RESERVED_KEYWORDS, TYPE_ORDER, TypeKind, basic_type,
    default_precision, is_keyword, is_reserved, keyword,
};
pub use legacy::{
    LEGACY_FUNCTIONS, LEGACY_VARIABLES, LegacyFunction, LegacyVariable, legacy_function,
    legacy_variable,
};
pub use model::{
    BuiltinFunction, BuiltinVariable, DocRef, Family, FamilyId, Flow, Overload, Param,
    ParamDoc, Stage, StageMask, TypeRef,
};
pub use version::{
    Availability, DesktopMask, DesktopVersion, EsMask, EsVersion, Version,
};

/// The builtin function of that name, if GLSL has one.
///
/// The name alone — overload selection is the caller's job, and needs the
/// argument types this crate knows nothing about.
pub fn function(name: &str) -> Option<&'static BuiltinFunction> {
    FUNCTIONS.binary_search_by_key(&name, |f| f.name).ok().map(|i| &FUNCTIONS[i])
}

/// The predeclared `gl_*` variable of that name, if there is one.
pub fn variable(name: &str) -> Option<&'static BuiltinVariable> {
    VARIABLES.binary_search_by_key(&name, |v| v.name).ok().map(|i| &VARIABLES[i])
}

/// The generic family of that name — `genType`, `gvec4`, `gsampler2D`.
pub fn family(name: &str) -> Option<FamilyId> {
    FAMILIES.binary_search_by_key(&name, |f| f.name).ok().map(|i| FamilyId(i as u16))
}

/// Whether the name is predeclared at all, as a function or a variable. The
/// cheap first question for a completion or a diagnostic.
pub fn is_builtin(name: &str) -> bool {
    function(name).is_some() || variable(name).is_some()
}

/// Every builtin that exists in this version, functions then variables, in
/// name order — the completion list, already filtered.
pub fn visible_in(version: Version) -> impl Iterator<Item = Predeclared> + use<> {
    let functions = FUNCTIONS
        .iter()
        .filter(move |f| f.available_in(version))
        .map(Predeclared::Function);
    let variables = VARIABLES
        .iter()
        .filter(move |v| v.available_in(version))
        .map(Predeclared::Variable);
    functions.chain(variables)
}

/// One of the two things GLSL predeclares.
#[derive(Debug, Clone, Copy)]
pub enum Predeclared {
    Function(&'static BuiltinFunction),
    Variable(&'static BuiltinVariable),
}

impl Predeclared {
    pub fn name(self) -> &'static str {
        match self {
            Predeclared::Function(f) => f.name,
            Predeclared::Variable(v) => v.name,
        }
    }

    pub fn doc(self) -> DocRef {
        match self {
            Predeclared::Function(f) => f.doc,
            Predeclared::Variable(v) => v.doc,
        }
    }
}
