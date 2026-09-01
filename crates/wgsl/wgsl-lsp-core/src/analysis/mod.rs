//! The naga side of the server.
//!
//! [`wgsl_syntax`] answers structural questions about any input. naga answers
//! *semantic* ones — what type is this, is this program valid — but only for
//! sources it can parse in full. This module is the boundary: it runs naga,
//! keeps what came back, and records why it did not run when it did not.
//!
//! Everything here is `Option`-shaped for that reason. A feature that wants a
//! naga answer asks for one and has a syntax-only fallback ready.

pub mod dialect;
pub mod stage;
pub mod types;

use std::rc::Rc;

use analyzer_core::spans::ByteSpan;
use naga::valid::{Capabilities, ValidationFlags, Validator};
use naga::{Module, ShaderStage};
use wgsl_syntax::Language;

use dialect::Dialect;
use stage::{resolve_stage, stage_name};

/// The stage label reported for a GLSL file naga's front end cannot parse.
pub const UNSUPPORTED_STAGE: &str = "unsupported";

/// Something naga found wrong with the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub span: ByteSpan,
    pub message: String,
}

/// What naga made of a source.
#[derive(Debug)]
pub struct Analysis {
    /// The parsed module, when naga could parse it.
    ///
    /// Shared rather than owned so [`crate::state::Document`] can hold on to
    /// the last one that parsed without copying an arena per keystroke.
    pub module: Option<Rc<Module>>,
    /// The GLSL stage the source was parsed as. `None` for WGSL, which has no
    /// stages, and for a GLSL stage naga does not implement.
    pub stage: Option<ShaderStage>,
    /// How the stage is reported to the user: a stage name, or
    /// [`UNSUPPORTED_STAGE`].
    pub stage_label: Option<&'static str>,
    /// Why naga was not run, in words the user can act on.
    ///
    /// A GLSL ES source, or an OpenGL-style one, is *valid GLSL* that naga
    /// simply does not implement. Reporting its every line as an error would be
    /// worse than saying nothing, so validation is skipped and the reason
    /// recorded instead — see [`dialect`].
    pub skipped: Option<String>,
    pub problems: Vec<Problem>,
}

impl Analysis {
    /// Run naga over a source.
    ///
    /// `extension` is the file's extension without the dot, used to pick the
    /// GLSL stage; pass an empty string when there is none. `dialect` is the
    /// GLSL a source is to be judged against, and is ignored for WGSL.
    pub fn run(source: &str, language: Language, extension: &str, dialect: Dialect) -> Analysis {
        match language {
            Language::Wgsl => wgsl(source),
            Language::Glsl => glsl(source, extension, dialect),
        }
    }

    /// An analysis that ran nothing, for a document naga has no opinion on.
    pub fn empty() -> Analysis {
        Analysis {
            module: None,
            stage: None,
            stage_label: None,
            skipped: None,
            problems: Vec::new(),
        }
    }
}

fn wgsl(source: &str) -> Analysis {
    let module = match naga::front::wgsl::parse_str(source) {
        Ok(module) => module,
        Err(error) => {
            // `labels()` carries a span per label, which is what makes the
            // squiggle land on the offending token rather than the file start.
            let message = error.emit_to_string(source);
            let problems: Vec<Problem> = error
                .labels()
                .filter_map(|(span, _)| span.to_range())
                .map(|range| Problem {
                    span: ByteSpan::from_usize(range.start, range.end),
                    message: message.clone(),
                })
                .collect();
            return Analysis {
                module: None,
                stage: None,
                stage_label: None,
                skipped: None,
                problems: if problems.is_empty() {
                    vec![Problem { span: ByteSpan::EMPTY, message }]
                } else {
                    problems
                },
            };
        }
    };

    let problems = validate(&module, source);
    Analysis {
        module: Some(Rc::new(module)),
        stage: None,
        stage_label: None,
        skipped: None,
        problems,
    }
}

fn glsl(source: &str, extension: &str, dialect: Dialect) -> Analysis {
    let Some(stage) = resolve_stage(source, extension) else {
        return Analysis {
            module: None,
            stage: None,
            stage_label: Some(UNSUPPORTED_STAGE),
            skipped: Some(
                "naga's GLSL front end implements the vertex, fragment and compute stages only"
                    .to_string(),
            ),
            problems: Vec::new(),
        };
    };
    let stage_label = Some(stage_name(stage));

    if let Some(reason) = dialect::skip_reason(source, dialect) {
        return Analysis {
            module: None,
            stage: Some(stage),
            stage_label,
            skipped: Some(reason),
            problems: Vec::new(),
        };
    }

    let mut frontend = naga::front::glsl::Frontend::default();
    let module = match frontend.parse(&naga::front::glsl::Options::from(stage), source) {
        Ok(module) => module,
        Err(errors) => {
            let problems = errors
                .errors
                .iter()
                .map(|error| Problem {
                    span: error
                        .meta
                        .to_range()
                        .map(|range| ByteSpan::from_usize(range.start, range.end))
                        .unwrap_or(ByteSpan::EMPTY),
                    message: error.kind.to_string(),
                })
                .collect();
            return Analysis {
                module: None,
                stage: Some(stage),
                stage_label,
                skipped: None,
                problems,
            };
        }
    };

    let problems = validate(&module, source);
    Analysis {
        module: Some(Rc::new(module)),
        stage: Some(stage),
        stage_label,
        skipped: None,
        problems,
    }
}

/// Run the shared validator, mapping each reported span to a problem.
fn validate(module: &Module, source: &str) -> Vec<Problem> {
    let mut validator = Validator::new(ValidationFlags::all(), Capabilities::all());
    let Err(error) = validator.validate(module) else {
        return Vec::new();
    };

    let message = error.emit_to_string(source);
    let problems: Vec<Problem> = error
        .spans()
        .filter_map(|(span, _)| span.to_range())
        .map(|range| Problem {
            span: ByteSpan::from_usize(range.start, range.end),
            message: message.clone(),
        })
        .collect();
    if problems.is_empty() {
        vec![Problem { span: ByteSpan::EMPTY, message }]
    } else {
        problems
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VERTEX: &str = "#version 450\n\
        layout(location = 0) in vec3 position;\n\
        void main() { gl_Position = vec4(position, 1.0); }\n";

    /// Most tests here are not about the dialect, and `Auto` is what a user
    /// with default settings gets.
    fn run(source: &str, language: Language, extension: &str) -> Analysis {
        Analysis::run(source, language, extension, Dialect::Auto)
    }

    #[test]
    fn a_valid_wgsl_module_produces_no_problems() {
        let analysis = run(
            "@fragment fn main() -> @location(0) vec4f { return vec4f(1.0); }",
            Language::Wgsl,
            "wgsl",
        );
        assert!(analysis.problems.is_empty(), "{:?}", analysis.problems);
        assert!(analysis.module.is_some());
    }

    /// The old implementation reported every WGSL parse error at the file
    /// start with zero length. `labels()` puts it on the token.
    #[test]
    fn a_wgsl_parse_error_lands_on_the_offending_token() {
        let source = "fn main() {\n    let x = ;\n}\n";
        let analysis = run(source, Language::Wgsl, "wgsl");
        assert!(analysis.module.is_none());
        let problem = &analysis.problems[0];
        assert!(problem.span.start > 0, "{problem:?}");
        assert!(source[..problem.span.start as usize].contains("let x"), "{problem:?}");
    }

    #[test]
    fn a_glsl_stage_comes_from_the_extension_then_the_source() {
        let analysis = run(VERTEX, Language::Glsl, "vert");
        assert_eq!(analysis.stage_label, Some("vertex"));
        assert!(analysis.problems.is_empty(), "{:?}", analysis.problems);

        // No extension to go on: sniffed from `gl_Position`.
        let analysis = run(VERTEX, Language::Glsl, "glsl");
        assert_eq!(analysis.stage_label, Some("vertex"));
    }

    #[test]
    fn a_stage_naga_cannot_parse_is_reported_rather_than_attempted() {
        let analysis = run("void main() {}", Language::Glsl, "tesc");
        assert_eq!(analysis.stage_label, Some(UNSUPPORTED_STAGE));
        assert!(analysis.problems.is_empty());
        assert!(analysis.skipped.is_some());
    }

    #[test]
    fn a_dialect_naga_does_not_implement_is_skipped_rather_than_flooded() {
        let source =
            "#version 300 es\nprecision mediump float;\nout vec4 c;\nvoid main() { c = vec4(1.0); }\n";
        let analysis = run(source, Language::Glsl, "frag");
        assert!(analysis.problems.is_empty());
        assert_eq!(analysis.stage_label, Some("fragment"));
        assert!(analysis.skipped.as_deref().is_some_and(|s| s.contains("300 es")));
    }

    /// The two errors an OpenGL-style shader used to collect: an implicit block
    /// binding, and a combined image sampler naga has no type for.
    #[test]
    fn an_opengl_style_shader_is_skipped_rather_than_flooded() {
        let source = "#version 450\n\
            uniform VertexInfo {\n  mat4 mvp;\n  mat4 model;\n}\nvertex_info;\n\
            uniform sampler2D albedoTexture;\n\
            void main() { gl_FragColor = vec4(1.0); }\n";

        let analysis = run(source, Language::Glsl, "frag");
        assert!(analysis.problems.is_empty(), "{:?}", analysis.problems);
        assert_eq!(analysis.stage_label, Some("fragment"));
        assert!(analysis.skipped.is_some());

        // Asking for Vulkan GLSL explicitly brings the errors back: someone
        // targeting `wgpu` wants to be told.
        let analysis = Analysis::run(source, Language::Glsl, "frag", Dialect::Vulkan);
        assert!(!analysis.problems.is_empty());
        assert!(analysis.skipped.is_none());
    }

    /// `opengl` switches naga off for a file it would otherwise have accepted.
    #[test]
    fn the_opengl_dialect_skips_even_a_vulkan_style_shader() {
        let analysis = Analysis::run(VERTEX, Language::Glsl, "vert", Dialect::OpenGl);
        assert!(analysis.problems.is_empty());
        assert_eq!(analysis.stage_label, Some("vertex"));
        assert!(analysis.skipped.is_some());
    }

    /// The example shaders the extension ships are the ones users try first.
    #[test]
    fn the_shipped_examples_validate() {
        const EXAMPLES: [(&str, Language, &str); 4] = [
            (include_str!("../../../../../extensions/wgsl-shader/examples/test.wgsl"),
             Language::Wgsl, "wgsl"),
            (include_str!("../../../../../extensions/wgsl-shader/examples/test.vert"),
             Language::Glsl, "vert"),
            (include_str!("../../../../../extensions/wgsl-shader/examples/test.frag"),
             Language::Glsl, "frag"),
            (include_str!("../../../../../extensions/wgsl-shader/examples/test.comp"),
             Language::Glsl, "comp"),
        ];
        for (source, language, extension) in EXAMPLES {
            let analysis = run(source, language, extension);
            assert!(analysis.problems.is_empty(), "{extension}: {:?}", analysis.problems);
        }
    }
}
