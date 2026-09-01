//! The WGSL front end: `naga::front::wgsl` parses, then the shared validator runs.

use naga::front::wgsl;

use crate::{tree_from_module, validate_module, Diagnostic, ShaderTree, ValidationResult};

pub fn validate(source: &str) -> ValidationResult {
    let module = match wgsl::parse_str(source) {
        Ok(module) => module,
        Err(err) => {
            let (line, col) = match err.location(source) {
                Some(loc) => (loc.line_number, loc.line_position),
                None => (1, 1),
            };
            return ValidationResult::failed(
                vec![Diagnostic {
                    message: err.emit_to_string(source),
                    line,
                    col,
                    length: 0,
                }],
                None,
            );
        }
    };

    let errors = validate_module(&module, source);
    if errors.is_empty() {
        ValidationResult::ok(None)
    } else {
        ValidationResult::failed(errors, None)
    }
}

pub fn tree(source: &str) -> ShaderTree {
    match wgsl::parse_str(source) {
        Ok(module) => tree_from_module(&module),
        Err(_) => ShaderTree::default(),
    }
}
