use naga::front::wgsl;
use naga::valid::{Capabilities, ValidationFlags, Validator};
use serde::Serialize;
use wasm_bindgen::prelude::*;

#[derive(Serialize)]
struct ValidationResult {
    ok: bool,
    errors: Vec<Diagnostic>,
}

#[derive(Serialize)]
struct Diagnostic {
    message: String,
    line: u32,
    col: u32,
    length: u32,
}

#[derive(Serialize, Default)]
struct WgslTree {
    types: Vec<String>,
    global_variables: Vec<String>,
    functions: Vec<String>,
}

#[wasm_bindgen(start)]
fn init() {
    console_error_panic_hook::set_once();
}

#[wasm_bindgen]
pub fn validate_wgsl(source: &str) -> String {
    let result = validate_impl(source);
    serde_json::to_string(&result).unwrap_or_else(|_| r#"{"ok":false,"errors":[]}"#.to_string())
}

fn validate_impl(source: &str) -> ValidationResult {
    let module = match wgsl::parse_str(source) {
        Ok(m) => m,
        Err(err) => {
            let message = err.emit_to_string(source);
            let loc = err.location(source);
            let (line, col) = match loc {
                Some(l) => (l.line_number, l.line_position),
                None => (1, 1),
            };
            return ValidationResult {
                ok: false,
                errors: vec![Diagnostic {
                    message,
                    line,
                    col,
                    length: 0,
                }],
            };
        }
    };

    let mut validator = Validator::new(ValidationFlags::all(), Capabilities::all());
    match validator.validate(&module) {
        Ok(_) => ValidationResult {
            ok: true,
            errors: vec![],
        },
        Err(error) => {
            let mut errors = Vec::new();
            let message = error.emit_to_string(source);
            for (span, _) in error.spans() {
                let loc = span.location(source);
                errors.push(Diagnostic {
                    message: message.clone(),
                    line: loc.line_number,
                    col: loc.line_position,
                    length: loc.length,
                });
            }
            if errors.is_empty() {
                errors.push(Diagnostic {
                    message,
                    line: 1,
                    col: 1,
                    length: 0,
                });
            }
            ValidationResult { ok: false, errors }
        }
    }
}

#[wasm_bindgen]
pub fn get_wgsl_tree(source: &str) -> String {
    let tree = get_tree_impl(source);
    serde_json::to_string(&tree).unwrap_or_else(|_| {
        r#"{"types":[],"global_variables":[],"functions":[]}"#.to_string()
    })
}

fn get_tree_impl(source: &str) -> WgslTree {
    let module = match wgsl::parse_str(source) {
        Ok(m) => m,
        Err(_) => return WgslTree::default(),
    };

    let mut types = Vec::new();
    let mut global_variables = Vec::new();
    let mut functions = Vec::new();

    for (_, ty) in module.types.iter() {
        if let Some(name) = &ty.name {
            types.push(name.clone());
        }
    }
    for (_, var) in module.global_variables.iter() {
        if let Some(name) = &var.name {
            global_variables.push(name.clone());
        }
    }
    for (_, f) in module.functions.iter() {
        if let Some(name) = &f.name {
            functions.push(name.clone());
        }
    }

    WgslTree {
        types,
        global_variables,
        functions,
    }
}
