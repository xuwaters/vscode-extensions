//! Shader validation and outline extraction for the WGSL/GLSL VS Code
//! extension, compiled to WebAssembly.
//!
//! Both languages go through [naga](https://github.com/gfx-rs/wgpu): the WGSL
//! or GLSL front end parses the source into a `naga::Module`, then the shared
//! validator checks it. Every entry point returns JSON so the extension host
//! can `JSON.parse` the result without a binding layer.

mod glsl;
mod stage;
mod wgsl;

use naga::valid::{Capabilities, ValidationFlags, Validator};
use naga::Module;
use serde::Serialize;
use wasm_bindgen::prelude::*;

#[derive(Serialize)]
pub struct ValidationResult {
    ok: bool,
    errors: Vec<Diagnostic>,
    /// The GLSL stage the source was parsed as, or `"unsupported"` for a stage
    /// naga cannot parse. Absent for WGSL, which has no stages.
    #[serde(skip_serializing_if = "Option::is_none")]
    stage: Option<String>,
}

impl ValidationResult {
    fn ok(stage: Option<String>) -> Self {
        Self {
            ok: true,
            errors: Vec::new(),
            stage,
        }
    }

    fn failed(errors: Vec<Diagnostic>, stage: Option<String>) -> Self {
        Self {
            ok: false,
            errors,
            stage,
        }
    }
}

#[derive(Serialize)]
pub struct Diagnostic {
    message: String,
    line: u32,
    col: u32,
    length: u32,
}

#[derive(Serialize, Default)]
pub struct ShaderTree {
    types: Vec<String>,
    global_variables: Vec<String>,
    functions: Vec<String>,
}

/// Run the shared validator over a parsed module, mapping each reported span to
/// a diagnostic. Errors without a span are reported at the start of the file.
fn validate_module(module: &Module, source: &str) -> Vec<Diagnostic> {
    let mut validator = Validator::new(ValidationFlags::all(), Capabilities::all());
    let error = match validator.validate(module) {
        Ok(_) => return Vec::new(),
        Err(error) => error,
    };

    let message = error.emit_to_string(source);
    let mut errors: Vec<Diagnostic> = error
        .spans()
        .map(|(span, _)| {
            let loc = span.location(source);
            Diagnostic {
                message: message.clone(),
                line: loc.line_number,
                col: loc.line_position,
                length: loc.length,
            }
        })
        .collect();
    if errors.is_empty() {
        errors.push(Diagnostic {
            message,
            line: 1,
            col: 1,
            length: 0,
        });
    }
    errors
}

/// Names declared by a module, for completion and the outline.
fn tree_from_module(module: &Module) -> ShaderTree {
    ShaderTree {
        types: module
            .types
            .iter()
            .filter_map(|(_, ty)| ty.name.clone())
            .collect(),
        global_variables: module
            .global_variables
            .iter()
            .filter_map(|(_, var)| var.name.clone())
            .collect(),
        functions: module
            .functions
            .iter()
            .filter_map(|(_, f)| f.name.clone())
            .collect(),
    }
}

fn to_json<T: Serialize>(value: &T, fallback: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| fallback.to_string())
}

const VALIDATION_FALLBACK: &str = r#"{"ok":false,"errors":[]}"#;
const TREE_FALLBACK: &str = r#"{"types":[],"global_variables":[],"functions":[]}"#;

#[wasm_bindgen(start)]
fn init() {
    console_error_panic_hook::set_once();
}

#[wasm_bindgen]
pub fn validate_wgsl(source: &str) -> String {
    to_json(&wgsl::validate(source), VALIDATION_FALLBACK)
}

#[wasm_bindgen]
pub fn get_wgsl_tree(source: &str) -> String {
    to_json(&wgsl::tree(source), TREE_FALLBACK)
}

/// Validate GLSL. `extension` is the source file's extension without the dot
/// (`"vert"`, `"frag"`, `"glsl"`, …), used to pick the shader stage; pass an
/// empty string when there is none.
#[wasm_bindgen]
pub fn validate_glsl(source: &str, extension: &str) -> String {
    to_json(&glsl::validate(source, extension), VALIDATION_FALLBACK)
}

/// Names declared by a GLSL source, with the stage resolved as for
/// [`validate_glsl`].
#[wasm_bindgen]
pub fn get_glsl_tree(source: &str, extension: &str) -> String {
    to_json(&glsl::tree(source, extension), TREE_FALLBACK)
}

/// How a GLSL source is being treated, as
/// `{"stage": "vertex" | "fragment" | "compute" | "unsupported", "skipped"?: string}`.
/// `skipped` is present, and says why, when the source is outside the dialect
/// naga implements and so gets no diagnostics.
#[wasm_bindgen]
pub fn glsl_shader_info(source: &str, extension: &str) -> String {
    to_json(&glsl::info(source, extension), r#"{"stage":"unsupported"}"#)
}
