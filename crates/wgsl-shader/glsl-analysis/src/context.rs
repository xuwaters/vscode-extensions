//! Version, profile and stage — the context every availability question is
//! asked in (P4-07).
//!
//! Three facts decide what a name means, and each is known with a different
//! confidence:
//!
//! - **The version** is what `#version` says. It is the one fact the file
//!   states outright, so version gating is allowed to produce errors. A number
//!   no profile has (`#version 200`) leaves [`Context::version_known`] false and
//!   switches version gating off entirely rather than guessing a neighbour.
//! - **The profile** is `core`, `compatibility` or `es`. It decides whether the
//!   legacy table's names are in force past their core masks.
//! - **The stage** comes from the file's extension, which the host passes in.
//!   When it does not, the stage is *guessed* from the file's own contents, and
//!   a guess never produces an error — only [`Context::stage_known`] unlocks
//!   the stage rules.
//!
//! The whole point is that a wrong guess must not paint a valid file red.

use glsl_spec::{Availability, Stage, Version};
use glsl_syntax::{Preprocessed, Profile};

/// What the host knows about a document before analysis starts.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// The stage, when the file extension or the client settled it. `None`
    /// means "guess, and keep the guess to yourself".
    pub stage: Option<Stage>,
    /// The version to assume for a file that declares no `#version`.
    ///
    /// The spec says such a file is GLSL 1.10, and that is what `None` gives.
    /// A host may know better: a project whose shaders are assembled from
    /// `#version`-less fragments, or compiled with `-DGL_ES` and a version
    /// supplied on the command line, is a real thing, and holding those files
    /// to 1.10 paints every modern name red. Added by RFC 012 P5-09 to back
    /// the `glsl.defaultVersion` setting.
    pub default_version: Option<Version>,
}

impl Options {
    /// The stage a shader file extension names — `.frag` is a fragment shader.
    /// The set matches the extension's `package.json` plus the stages glslang's
    /// corpus uses.
    pub fn stage_for_extension(extension: &str) -> Option<Stage> {
        match extension {
            "vert" | "vs" | "vsh" | "glslv" => Some(Stage::Vertex),
            "frag" | "fs" | "fsh" | "glslf" => Some(Stage::Fragment),
            "comp" => Some(Stage::Compute),
            "geom" | "gsh" => Some(Stage::Geometry),
            "tesc" => Some(Stage::TessControl),
            "tese" => Some(Stage::TessEvaluation),
            _ => None,
        }
    }

    /// The options for a file with this name.
    pub fn for_path(path: &str) -> Options {
        let extension = path.rsplit('.').next().unwrap_or("");
        Options {
            stage: Options::stage_for_extension(extension),
            ..Options::default()
        }
    }
}

/// The dialect a file is analysed as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Context {
    /// The version in force. Falls back to GLSL 1.10, which is what the spec
    /// says a file with no `#version` is.
    pub version: Version,
    /// Whether `version` came from a `#version` this crate recognised. When
    /// false, every availability rule stays silent.
    pub version_known: bool,
    pub profile: Profile,
    /// The stage in force, guessed when the host supplied none.
    pub stage: Stage,
    /// Whether `stage` was told to us rather than guessed. Stage rules only
    /// fire when this holds.
    pub stage_known: bool,
}

impl Default for Context {
    fn default() -> Context {
        Context {
            version: Version::DEFAULT,
            version_known: false,
            profile: Profile::Core,
            stage: Stage::Fragment,
            stage_known: false,
        }
    }
}

impl Context {
    /// Read the context off a preprocessed file and the host's options.
    pub fn new(pp: &Preprocessed, options: &Options) -> Context {
        let profile = pp.profile();
        let (version, version_known) = match &pp.directives.version {
            Some(directive) => {
                let es = profile == Profile::Es;
                match u16::try_from(directive.number)
                    .ok()
                    .and_then(|number| Version::from_directive(number, es))
                {
                    Some(version) => (version, true),
                    // A number no profile has. Analysing it as some neighbour
                    // would invent availability answers, so gating goes off.
                    None => (Version::DEFAULT, false),
                }
            }
            // No `#version` at all is 1.10 by the spec, and that *is* known —
            // it is the rule, not a guess. The host may substitute its own
            // default; either way the answer is known, and availability rules
            // apply against it.
            None => (options.default_version.unwrap_or(Version::DEFAULT), true),
        };
        let (stage, stage_known) = match options.stage {
            Some(stage) => (stage, true),
            None => (guess_stage(pp), false),
        };
        Context { version, version_known, profile, stage, stage_known }
    }

    /// Whether the compatibility profile's extra names are in force.
    ///
    /// `#version 150` and older have no profiles at all and predeclare the
    /// whole legacy surface, so they count too.
    pub fn compatibility(&self) -> bool {
        self.profile == Profile::Compatibility
            || matches!(self.version, Version::Desktop(v) if v.number() <= 150)
    }

    /// Whether something with this availability exists here.
    ///
    /// `compatibility` is the entry's own "the compatibility profile keeps
    /// this" flag. When the version is not known, everything is available:
    /// silence beats a false "not available in this version".
    pub fn available(&self, availability: Availability, compatibility: bool) -> bool {
        if !self.version_known {
            return true;
        }
        if compatibility && self.compatibility() {
            return true;
        }
        availability.contains(self.version)
    }

    /// The version as a diagnostic writes it: `4.50`, `3.00 es`.
    pub fn version_label(&self) -> String {
        self.version.label()
    }
}

/// Which stage a file looks like, from the names it uses.
///
/// Only ever consulted when the host could not say, and only ever used to make
/// *answers* better (a hover for `gl_FragCoord`), never to make an error. The
/// order is by how decisive the signal is.
fn guess_stage(pp: &Preprocessed) -> Stage {
    let mut saw_fragment = false;
    let mut saw_vertex = false;
    for token in &pp.tokens {
        match token.text.as_str() {
            "gl_FragColor" | "gl_FragData" | "gl_FragDepth" | "gl_FragCoord"
            | "gl_FrontFacing" | "discard" => saw_fragment = true,
            "gl_VertexID" | "gl_VertexIndex" | "gl_InstanceID" | "gl_InstanceIndex" => {
                saw_vertex = true
            }
            "gl_Position" => saw_vertex = true,
            "gl_GlobalInvocationID" | "gl_LocalInvocationID" | "gl_WorkGroupID"
            | "gl_NumWorkGroups" | "gl_LocalInvocationIndex" | "local_size_x" => {
                return Stage::Compute;
            }
            "gl_TessCoord" | "gl_TessLevelInner" | "gl_TessLevelOuter" => {
                return Stage::TessEvaluation;
            }
            "EmitVertex" | "EndPrimitive" | "gl_PrimitiveIDIn" => return Stage::Geometry,
            _ => {}
        }
    }
    match (saw_fragment, saw_vertex) {
        (true, false) => Stage::Fragment,
        (false, true) => Stage::Vertex,
        // A file that says neither, or both, gets the stage that predeclares
        // the most: every fragment builtin is also readable from the others in
        // at least one dialect, and being wrong here costs nothing but a hover.
        _ => Stage::Fragment,
    }
}
