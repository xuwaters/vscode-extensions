//! The two builtin tables, read as one (P4-06, P4-07).
//!
//! `glsl-spec` carries the generated tables — every builtin docs.gl documents —
//! and, beside them, the hand-written legacy table decision 0007 added for the
//! compatibility and ES 1.00 surface docs.gl documents nowhere. A caller should
//! not care which one an answer came from, so every lookup here returns one
//! enum and every accessor works the same on both.
//!
//! The one thing that *is* different is availability: a legacy entry carries a
//! "the compatibility profile keeps this" flag on top of its version masks,
//! which [`crate::context::Context::available`] takes as a second argument.

use std::sync::OnceLock;

use glsl_spec::{
    BuiltinFunction, BuiltinVariable, FamilyId, LegacyFunction, LegacyVariable, Stage, Version,
};

use crate::Target;
use crate::types::Type;

/// The [`Type`] of every member of every generic family, in the order
/// [`glsl_spec::Family::members`] lists them. `None` where the spelling names
/// something [`Type::from_name`] does not model, which is the case a caller
/// must skip exactly as it would have skipped a failed conversion.
///
/// Derived once per process rather than once per candidate. Overload
/// resolution binds a family by trying its members in turn, for every
/// candidate overload of every builtin call in the file, and re-deriving those
/// types from their spellings each time was the analysis layer's single
/// largest cost (RFC 012 P5-10). The tables are static, so the answer cannot
/// change; ~40 families of a handful of members each is a few kilobytes.
pub fn family_members(id: FamilyId) -> &'static [Option<Type>] {
    static MEMBERS: OnceLock<Vec<Vec<Option<Type>>>> = OnceLock::new();
    let all = MEMBERS.get_or_init(|| {
        glsl_spec::FAMILIES
            .iter()
            .map(|family| family.members.iter().map(|m| Type::from_name(m)).collect())
            .collect()
    });
    all.get(id.0 as usize).map_or(&[], |members| members.as_slice())
}

/// A builtin function, from either table.
#[derive(Debug, Clone, Copy)]
pub enum FoundFunction {
    Generated(&'static BuiltinFunction),
    Legacy(&'static LegacyFunction),
}

impl FoundFunction {
    pub fn function(self) -> &'static BuiltinFunction {
        match self {
            FoundFunction::Generated(f) => f,
            FoundFunction::Legacy(f) => &f.function,
        }
    }

    pub fn compatibility(self) -> bool {
        match self {
            FoundFunction::Generated(_) => false,
            FoundFunction::Legacy(f) => f.compatibility,
        }
    }

    pub fn target(self) -> Target {
        match self {
            FoundFunction::Generated(f) => Target::BuiltinFunction(f),
            FoundFunction::Legacy(f) => Target::LegacyFunction(f),
        }
    }

    /// The hand-written prose for a legacy entry; the generated pool's for the
    /// rest.
    pub fn doc(self) -> &'static str {
        match self {
            FoundFunction::Generated(f) => f.doc.text(),
            FoundFunction::Legacy(f) => f.doc,
        }
    }
}

/// A predeclared variable, from either table.
#[derive(Debug, Clone, Copy)]
pub enum FoundVariable {
    Generated(&'static BuiltinVariable),
    Legacy(&'static LegacyVariable),
}

impl FoundVariable {
    pub fn variable(self) -> &'static BuiltinVariable {
        match self {
            FoundVariable::Generated(v) => v,
            FoundVariable::Legacy(v) => &v.variable,
        }
    }

    pub fn compatibility(self) -> bool {
        match self {
            FoundVariable::Generated(_) => false,
            FoundVariable::Legacy(v) => v.compatibility,
        }
    }

    pub fn target(self) -> Target {
        match self {
            FoundVariable::Generated(v) => Target::BuiltinVariable(v),
            FoundVariable::Legacy(v) => Target::LegacyVariable(v),
        }
    }

    pub fn doc(self) -> &'static str {
        match self {
            FoundVariable::Generated(v) => v.doc.text(),
            FoundVariable::Legacy(v) => v.doc,
        }
    }
}

/// The builtin function of that name, generated table first.
pub fn lookup_function(name: &str) -> Option<FoundFunction> {
    glsl_spec::function(name)
        .map(FoundFunction::Generated)
        .or_else(|| glsl_spec::legacy_function(name).map(FoundFunction::Legacy))
}

/// The predeclared variable of that name, generated table first.
pub fn lookup_variable(name: &str) -> Option<FoundVariable> {
    glsl_spec::variable(name)
        .map(FoundVariable::Generated)
        .or_else(|| glsl_spec::legacy_variable(name).map(FoundVariable::Legacy))
}

/// Whether the name is predeclared at all, as a function or a variable.
pub fn is_builtin(name: &str) -> bool {
    lookup_function(name).is_some() || lookup_variable(name).is_some()
}

/// The type a table spells as a string: `vec4`, `float[]`, `int[4]`.
///
/// A name no table here knows — `gl_LightSourceParameters`, and the other
/// fixed-function structs the legacy table names but does not model — becomes
/// [`Type::Unknown`], which is exactly right: the variable resolves, hover
/// works, and every rule downstream stays quiet about it.
pub fn type_of(spelling: &'static str) -> Type {
    match spelling.split_once('[') {
        Some((base, rest)) => {
            let size = rest.trim_end_matches(']').parse::<u32>().ok();
            Type::Array(Box::new(type_of_base(base)), size)
        }
        None => type_of_base(spelling),
    }
}

fn type_of_base(name: &str) -> Type {
    Type::from_name(name).unwrap_or(Type::Unknown)
}

/// Whether the tables have anything at all to say about this profile.
///
/// docs.gl has one set of pages per profile, and where a page is missing the
/// mask comes back empty — which means "not documented here", not "not in the
/// language". `gl_SampleMask` has no ES page and is in ES 3.20 all the same. So
/// an availability error is only ever raised against a profile the entry has
/// *some* version in; the other profile stays silent.
pub fn profile_is_covered(availability: glsl_spec::Availability, version: Version) -> bool {
    if version.is_es() {
        !availability.es.is_empty()
    } else {
        !availability.desktop.is_empty()
    }
}

/// Whether a builtin variable belongs to exactly one stage, and one this crate
/// is confident about.
///
/// The generated stage masks come from prose, and prose is incomplete: reading
/// `gl_ClipDistance` in a fragment shader has been legal since 4.30 and the
/// reference page does not say so. So a stage error is only ever raised for a
/// variable that belongs to a single stage *and* that stage is fragment or
/// compute — the two whose builtins genuinely appear nowhere else.
pub fn is_stage_exclusive(variable: &BuiltinVariable) -> bool {
    if variable.stages.bits().count_ones() != 1 {
        return false;
    }
    variable.stages.contains(Stage::Fragment) || variable.stages.contains(Stage::Compute)
}

/// What to say after "this does not exist here", when there is something
/// useful to say.
pub fn replacement_hint(name: &str, version: Version) -> String {
    let modern = match name {
        "texture1D" | "texture2D" | "texture3D" | "textureCube" => Some("texture"),
        "texture1DProj" | "texture2DProj" | "texture3DProj" => Some("textureProj"),
        "texture1DLod" | "texture2DLod" | "texture3DLod" | "textureCubeLod" => {
            Some("textureLod")
        }
        "texture2DProjLod" | "texture1DProjLod" | "texture3DProjLod" => {
            Some("textureProjLod")
        }
        "shadow1D" | "shadow2D" => Some("texture"),
        "gl_FragColor" | "gl_FragData" => {
            return "; declare an 'out' variable instead".to_string();
        }
        _ => None,
    };
    if let Some(modern) = modern {
        return format!("; use '{modern}'");
    }
    // The other direction: a modern name in an old shader. Which legacy
    // spelling is right depends on the sampler, which this hint does not have,
    // so it names the 2D one — by far the most common — and says "spells it".
    let _ = version;
    let legacy = match name {
        "texture" => Some("texture2D"),
        "textureProj" => Some("texture2DProj"),
        "textureLod" => Some("texture2DLod"),
        _ => None,
    };
    match legacy {
        Some(legacy) => format!("; this version spells it '{legacy}'"),
        None => String::new(),
    }
}
