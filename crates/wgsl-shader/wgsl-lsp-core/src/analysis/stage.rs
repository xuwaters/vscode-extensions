//! Working out which shader stage a GLSL source belongs to.
//!
//! GLSL has no in-language way to say "this is a fragment shader", so the stage
//! has to come from somewhere else. In order of authority: the
//! `#pragma shader_stage(…)` directive that `glslc` defines, then the file
//! extension.
//!
//! When neither says, this module returns `None` and the *analyzer* guesses
//! from the builtins the source uses — and marks the guess as a guess, so no
//! stage-availability rule ever fires on it
//! ([`glsl_analysis::Context::stage_known`]). A wrong guess must never paint a
//! valid file red; it may only make a hover better.

use glsl_spec::Stage;

/// What a GLSL file's extension says about its stage.
///
/// `Unknown` covers the extension-less `.glsl` case and anything we do not
/// recognise. Every stage GLSL has is now a stage the analyzer implements, so
/// there is no "unsupported" answer any more.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageHint {
    Known(Stage),
    Unknown,
}

/// Map a lower-case file extension (no dot) to the stage it conventionally
/// means.
///
/// The names follow the `glslang` / `glslc` conventions plus the short forms
/// (`.vs`, `.fs`) that engines and sample code tend to use.
pub fn hint_from_extension(extension: &str) -> StageHint {
    match extension {
        "vert" | "vs" | "vsh" | "vshader" | "vertexshader" | "glslv" | "vertex" => {
            StageHint::Known(Stage::Vertex)
        }
        "frag" | "fs" | "fsh" | "fshader" | "fragmentshader" | "glslf" | "fragment"
        | "pixel" => StageHint::Known(Stage::Fragment),
        "comp" | "cs" | "csh" | "compute" => StageHint::Known(Stage::Compute),
        "geom" | "geometry" | "gsh" | "gshader" => StageHint::Known(Stage::Geometry),
        "tesc" => StageHint::Known(Stage::TessControl),
        "tese" => StageHint::Known(Stage::TessEvaluation),
        _ => StageHint::Unknown,
    }
}

/// The stage named by a `#pragma shader_stage(…)` directive, if the source has
/// one.
///
/// `glslc` gives this directive precedence over the file extension, and so do
/// we. Read off the raw text rather than the preprocessed pragma record so the
/// answer is available *before* preprocessing, which is what needs it.
pub fn stage_from_pragma(source: &str) -> Option<Stage> {
    for line in source.lines() {
        let line = line.trim_start();
        let Some(rest) = line.strip_prefix('#') else {
            continue;
        };
        let Some(rest) = rest.trim_start().strip_prefix("pragma") else {
            continue;
        };
        let Some(rest) = rest.trim_start().strip_prefix("shader_stage") else {
            continue;
        };
        let Some(rest) = rest.trim_start().strip_prefix('(') else {
            continue;
        };
        let Some((name, _)) = rest.split_once(')') else {
            continue;
        };
        return stage_named(name.trim());
    }
    None
}

/// The stage a `#pragma shader_stage` argument names.
///
/// `glslc`'s spellings, plus the ones the reference pages use.
pub fn stage_named(name: &str) -> Option<Stage> {
    match name {
        "vertex" | "vert" => Some(Stage::Vertex),
        "fragment" | "frag" => Some(Stage::Fragment),
        "compute" | "comp" => Some(Stage::Compute),
        "geometry" | "geom" => Some(Stage::Geometry),
        "tesscontrol" | "tessellation control" | "tesc" => Some(Stage::TessControl),
        "tesseval" | "tessevaluation" | "tessellation evaluation" | "tese" => {
            Some(Stage::TessEvaluation)
        }
        _ => None,
    }
}

/// The stage `source` declares, given the file extension it was read from.
///
/// `None` means nothing said, and the analyzer should guess.
pub fn resolve_stage(source: &str, extension: &str) -> Option<Stage> {
    if let Some(stage) = stage_from_pragma(source) {
        return Some(stage);
    }
    match hint_from_extension(extension) {
        StageHint::Known(stage) => Some(stage),
        StageHint::Unknown => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_map_to_their_stage() {
        assert_eq!(hint_from_extension("vert"), StageHint::Known(Stage::Vertex));
        assert_eq!(hint_from_extension("fs"), StageHint::Known(Stage::Fragment));
        assert_eq!(hint_from_extension("comp"), StageHint::Known(Stage::Compute));
        // Stages naga's front end never implemented and ours does.
        assert_eq!(hint_from_extension("geom"), StageHint::Known(Stage::Geometry));
        assert_eq!(hint_from_extension("tesc"), StageHint::Known(Stage::TessControl));
        assert_eq!(hint_from_extension("glsl"), StageHint::Unknown);
    }

    #[test]
    fn pragma_beats_the_extension() {
        let source = "#version 450\n#pragma shader_stage(compute)\nvoid main() {}\n";
        assert_eq!(resolve_stage(source, "vert"), Some(Stage::Compute));
        assert_eq!(resolve_stage(source, "geom"), Some(Stage::Compute));
    }

    #[test]
    fn pragma_spellings() {
        assert_eq!(
            stage_from_pragma("  #  pragma  shader_stage( fragment ) // why not"),
            Some(Stage::Fragment)
        );
        assert_eq!(
            stage_from_pragma("#pragma shader_stage(geometry)"),
            Some(Stage::Geometry)
        );
        assert_eq!(stage_from_pragma("#pragma shader_stage(nonsense)"), None);
        assert_eq!(stage_from_pragma("#version 450\nvoid main() {}"), None);
    }

    /// Nothing to go on is `None`, not a guess: the guess belongs to the
    /// analyzer, which also records that it guessed.
    #[test]
    fn an_unmarked_file_names_no_stage() {
        assert_eq!(resolve_stage("void main() { gl_FragColor = vec4(1.0); }", "glsl"), None);
    }
}
