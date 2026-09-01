//! The GLSL front end.
//!
//! GLSL needs a shader stage before it can be parsed at all (see [`crate::stage`]),
//! and naga's front end covers only part of the language: the vertex, fragment
//! and compute stages, and the 440, 450 and 460 core versions. Anything outside
//! that is reported as skipped rather than drowned in parse errors that only say
//! naga does not implement the dialect.

use naga::front::glsl::{Frontend, Options};
use serde::Serialize;

use crate::stage::{resolve_stage, stage_name};
use crate::{tree_from_module, validate_module, Diagnostic, ShaderTree, ValidationResult};

const UNSUPPORTED: &str = "unsupported";

/// The `#version` declarations naga's GLSL front end accepts.
const SUPPORTED_VERSIONS: [u32; 3] = [440, 450, 460];

/// What the editor needs to explain how a GLSL file is being treated.
#[derive(Serialize)]
pub struct ShaderInfo {
    stage: String,
    /// Why validation was skipped, if it was.
    #[serde(skip_serializing_if = "Option::is_none")]
    skipped: Option<String>,
}

pub fn resolved_stage_name(source: &str, extension: &str) -> &'static str {
    match resolve_stage(source, extension) {
        Some(stage) => stage_name(stage),
        None => UNSUPPORTED,
    }
}

pub fn info(source: &str, extension: &str) -> ShaderInfo {
    ShaderInfo {
        stage: resolved_stage_name(source, extension).to_string(),
        skipped: skip_reason(source, extension),
    }
}

/// The `#version` line's number and profile, if the source declares one.
fn declared_version(source: &str) -> Option<(u32, Option<String>)> {
    for line in source.lines() {
        let rest = match line.trim_start().strip_prefix('#') {
            Some(rest) => rest.trim_start(),
            None => continue,
        };
        let rest = match rest.strip_prefix("version") {
            Some(rest) => rest.trim(),
            // Another directive; `#version` need not be the first line.
            None => continue,
        };
        let mut words = rest.split_whitespace();
        let number = words.next()?.parse().ok()?;
        return Some((number, words.next().map(str::to_string)));
    }
    None
}

/// Why this source cannot be validated, in words a user can act on.
fn skip_reason(source: &str, extension: &str) -> Option<String> {
    if resolve_stage(source, extension).is_none() {
        return Some(
            "naga's GLSL front end implements the vertex, fragment and compute stages only"
                .to_string(),
        );
    }
    let (version, profile) = declared_version(source)?;
    let profile_suffix = match profile.as_deref() {
        Some(profile) => format!(" {profile}"),
        None => String::new(),
    };
    if profile.as_deref() == Some("es") || !SUPPORTED_VERSIONS.contains(&version) {
        return Some(format!(
            "naga's GLSL front end accepts #version 440, 450 and 460 core; this file declares {version}{profile_suffix}"
        ));
    }
    None
}

pub fn validate(source: &str, extension: &str) -> ValidationResult {
    let stage = match resolve_stage(source, extension) {
        Some(stage) => stage,
        None => return ValidationResult::ok(Some(UNSUPPORTED.to_string())),
    };
    let stage_label = Some(stage_name(stage).to_string());

    // A dialect naga does not implement would report every line as an error.
    if skip_reason(source, extension).is_some() {
        return ValidationResult::ok(stage_label);
    }

    let mut frontend = Frontend::default();
    let module = match frontend.parse(&Options::from(stage), source) {
        Ok(module) => module,
        Err(errors) => {
            let diagnostics = errors
                .errors
                .iter()
                .map(|error| {
                    let loc = error.meta.location(source);
                    Diagnostic {
                        message: error.kind.to_string(),
                        line: loc.line_number,
                        col: loc.line_position,
                        length: loc.length,
                    }
                })
                .collect();
            return ValidationResult::failed(diagnostics, stage_label);
        }
    };

    let errors = validate_module(&module, source);
    if errors.is_empty() {
        ValidationResult::ok(stage_label)
    } else {
        ValidationResult::failed(errors, stage_label)
    }
}

pub fn tree(source: &str, extension: &str) -> ShaderTree {
    let stage = match resolve_stage(source, extension) {
        Some(stage) => stage,
        None => return ShaderTree::default(),
    };
    let mut frontend = Frontend::default();
    match frontend.parse(&Options::from(stage), source) {
        Ok(module) => tree_from_module(&module),
        Err(_) => ShaderTree::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VERTEX: &str = "#version 450\n\
        layout(location = 0) in vec3 position;\n\
        void main() { gl_Position = vec4(position, 1.0); }\n";

    const FRAGMENT: &str = "#version 450\n\
        layout(location = 0) out vec4 color;\n\
        void main() { color = vec4(1.0, 0.0, 0.0, 1.0); }\n";

    const COMPUTE: &str = "#version 450\n\
        layout(local_size_x = 64) in;\n\
        layout(std430, binding = 0) buffer Data { float values[]; };\n\
        void main() { values[gl_GlobalInvocationID.x] *= 2.0; }\n";

    fn json(result: &ValidationResult) -> serde_json::Value {
        serde_json::to_value(result).unwrap()
    }

    #[test]
    fn valid_shaders_pass_for_each_stage() {
        for (source, extension, stage) in [
            (VERTEX, "vert", "vertex"),
            (FRAGMENT, "frag", "fragment"),
            (COMPUTE, "comp", "compute"),
        ] {
            let result = validate(source, extension);
            assert!(result.ok, "{stage}: {:?}", json(&result));
            assert_eq!(result.stage.as_deref(), Some(stage));
        }
    }

    #[test]
    fn a_syntax_error_is_reported_where_it_is() {
        let source = "#version 450\nvoid main() { float x = ; }\n";
        let result = validate(source, "frag");
        assert!(!result.ok);
        assert!(!result.errors.is_empty());
        assert_eq!(result.errors[0].line, 2);
        assert!(result.errors[0].col > 1);
    }

    #[test]
    fn an_undeclared_name_is_an_error() {
        let source = "#version 450\nvoid main() { gl_Position = vec4(nope, 1.0); }\n";
        let result = validate(source, "vert");
        assert!(!result.ok);
        assert!(
            result.errors.iter().any(|e| e.message.contains("nope")),
            "{:?}",
            json(&result)
        );
    }

    #[test]
    fn a_bare_glsl_file_is_sniffed() {
        let result = validate(COMPUTE, "glsl");
        assert!(result.ok, "{:?}", json(&result));
        assert_eq!(result.stage.as_deref(), Some("compute"));
    }

    #[test]
    fn unsupported_stages_report_no_errors() {
        let result = validate("#version 450\nlayout(triangles) in;\nvoid main() {}\n", "geom");
        assert!(result.ok);
        assert_eq!(result.stage.as_deref(), Some("unsupported"));
        assert!(result.errors.is_empty());
        assert_eq!(resolved_stage_name("", "tese"), "unsupported");
    }

    #[test]
    fn a_dialect_naga_does_not_implement_is_skipped_rather_than_flooded() {
        // GLSL ES, as WebGL uses it: naga cannot parse it, and reporting every
        // line as an error would be worse than saying nothing.
        let source = "#version 300 es\nprecision mediump float;\nout vec4 c;\nvoid main() { c = vec4(1.0); }\n";
        let result = validate(source, "frag");
        assert!(result.ok);
        assert!(result.errors.is_empty());
        assert_eq!(result.stage.as_deref(), Some("fragment"));

        let info = info(source, "frag");
        assert_eq!(info.stage, "fragment");
        assert!(
            info.skipped.as_deref().is_some_and(|s| s.contains("300 es")),
            "{:?}",
            info.skipped
        );
    }

    #[test]
    fn supported_versions_are_not_skipped() {
        for version in ["440", "450", "460"] {
            let source = format!("#version {version}\n{}", FRAGMENT.split_once('\n').unwrap().1);
            assert!(info(&source, "frag").skipped.is_none(), "{version}");
        }
        // No `#version` at all is fine too.
        assert!(info("void main() {}\n", "frag").skipped.is_none());
    }

    #[test]
    fn an_unsupported_stage_says_so_in_the_info() {
        let info = info("void main() {}\n", "tesc");
        assert_eq!(info.stage, "unsupported");
        assert!(info.skipped.is_some());
    }

    #[test]
    fn the_tree_lists_declared_names() {
        let source = "#version 450\n\
            struct Light { vec3 color; };\n\
            layout(binding = 0) uniform Light light;\n\
            float attenuate(float d) { return 1.0 / (d * d); }\n\
            layout(location = 0) out vec4 color;\n\
            void main() { color = vec4(light.color * attenuate(2.0), 1.0); }\n";
        let tree = tree(source, "frag");
        assert!(tree.types.iter().any(|t| t == "Light"), "{:?}", tree.types);
        assert!(
            tree.functions.iter().any(|f| f == "attenuate"),
            "{:?}",
            tree.functions
        );
        assert!(
            tree.global_variables.iter().any(|v| v == "light"),
            "{:?}",
            tree.global_variables
        );
    }

    /// The example shaders the extension ships are the ones users try first, so
    /// they had better survive the validator.
    #[test]
    fn the_shipped_examples_validate() {
        const EXAMPLES: [(&str, &str); 3] = [
            (
                include_str!("../../../extensions/wgsl-shader/examples/test.vert"),
                "vert",
            ),
            (
                include_str!("../../../extensions/wgsl-shader/examples/test.frag"),
                "frag",
            ),
            (
                include_str!("../../../extensions/wgsl-shader/examples/test.comp"),
                "comp",
            ),
        ];
        for (source, extension) in EXAMPLES {
            let result = validate(source, extension);
            assert!(result.ok, "{extension}: {:?}", json(&result));
        }
    }

    #[test]
    fn a_broken_source_yields_an_empty_tree() {
        let tree = tree("#version 450\nvoid main( {", "frag");
        assert!(tree.functions.is_empty());
    }
}
