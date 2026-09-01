//! The naga side of the server.
//!
//! naga is the WGSL authority: it type-checks, and it produces the diagnostics.
//! [`wgsl_syntax`] answers structural questions about any input, including the
//! half-typed line naga refuses; this module is the boundary between them.
//!
//! Everything here is `Option`-shaped for that reason. A feature that wants a
//! naga answer asks for one and has a syntax-only fallback ready.
//!
//! **WGSL only.** GLSL used to come through here too, against naga's Vulkan-
//! dialect front end, with [`stage`] telling it which stage to parse as and a
//! `dialect` module deciding when not to run it at all. RFC 012 phase 5 gave
//! GLSL its own analyzer ([`crate::glsl`]) and
//! [decision 0008](../../../../../docs/rfc/012-glsl-analyzer/decisions/0008-naga-glsl-in-dropped.md)
//! dropped naga's `glsl-in` feature; [`stage`] stays because *something* still
//! has to read `#pragma shader_stage`, and that something is now our own
//! analyzer's context.

pub mod stage;
pub mod types;

use std::rc::Rc;

use analyzer_core::spans::ByteSpan;
use naga::Module;
use naga::valid::{Capabilities, ValidationFlags, Validator};

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
    pub problems: Vec<Problem>,
}

impl Analysis {
    /// Run naga over a WGSL source.
    pub fn run(source: &str) -> Analysis {
        let module = match naga::front::wgsl::parse_str(source) {
            Ok(module) => module,
            Err(error) => {
                // `labels()` carries a span per label, which is what makes the
                // squiggle land on the offending token rather than the file
                // start.
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
                    problems: if problems.is_empty() {
                        vec![Problem { span: ByteSpan::EMPTY, message }]
                    } else {
                        problems
                    },
                };
            }
        };

        let problems = validate(&module, source);
        Analysis { module: Some(Rc::new(module)), problems }
    }

    /// An analysis that ran nothing, for a document naga has no opinion on —
    /// every GLSL one.
    pub fn empty() -> Analysis {
        Analysis { module: None, problems: Vec::new() }
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

    #[test]
    fn a_valid_wgsl_module_produces_no_problems() {
        let analysis =
            Analysis::run("@fragment fn main() -> @location(0) vec4f { return vec4f(1.0); }");
        assert!(analysis.problems.is_empty(), "{:?}", analysis.problems);
        assert!(analysis.module.is_some());
    }

    /// The old implementation reported every WGSL parse error at the file
    /// start with zero length. `labels()` puts it on the token.
    #[test]
    fn a_wgsl_parse_error_lands_on_the_offending_token() {
        let source = "fn main() {\n    let x = ;\n}\n";
        let analysis = Analysis::run(source);
        assert!(analysis.module.is_none());
        let problem = &analysis.problems[0];
        assert!(problem.span.start > 0, "{problem:?}");
        assert!(source[..problem.span.start as usize].contains("let x"), "{problem:?}");
    }

    /// The WGSL example the extension ships is the one users try first.
    #[test]
    fn the_shipped_wgsl_example_validates() {
        let source = include_str!("../../../../../extensions/wgsl-shader/examples/test.wgsl");
        let analysis = Analysis::run(source);
        assert!(analysis.problems.is_empty(), "{:?}", analysis.problems);
    }
}
