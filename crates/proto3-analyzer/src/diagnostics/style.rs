//! Style / naming-convention diagnostics. Off by default per §7 of the RFC
//! — enabled by the `proto3.diagnostics.style` setting.

use super::{DiagnosticCode, ProtoDiagnostic, Severity};
use crate::ast;
use crate::spans::ByteSpan;

#[derive(Debug, Clone, Copy, Default)]
pub struct StyleConfig {
    pub enabled: bool,
}

pub fn run_style_checks(file: &ast::File, cfg: StyleConfig) -> Vec<ProtoDiagnostic> {
    if !cfg.enabled {
        return Vec::new();
    }
    let mut out = Vec::new();
    for item in &file.items {
        match item {
            ast::TopLevelItem::Message(m) => visit_message(m, &mut out),
            ast::TopLevelItem::Enum(e) => visit_enum(e, &mut out),
            ast::TopLevelItem::Service(s) => visit_service(s, &mut out),
            ast::TopLevelItem::Extend(_) => {}
        }
    }
    out
}

fn warn(code: DiagnosticCode, message: String, span: ByteSpan) -> ProtoDiagnostic {
    ProtoDiagnostic::new(code, Severity::Warning, message, span)
}

fn visit_message(m: &ast::Message, out: &mut Vec<ProtoDiagnostic>) {
    if !is_upper_camel(&m.name.name) {
        out.push(warn(
            DiagnosticCode::StyleUpperCamel,
            format!("Message `{}` should use UpperCamelCase", m.name.name),
            m.name.span,
        ));
    }
    if m.fields.is_empty()
        && m.oneofs.is_empty()
        && m.nested_messages.is_empty()
        && m.nested_enums.is_empty()
    {
        out.push(warn(
            DiagnosticCode::StyleEmptyMessage,
            format!("Message `{}` is empty", m.name.name),
            m.name.span,
        ));
    }
    for f in &m.fields {
        if !is_lower_snake(&f.name.name) {
            out.push(warn(
                DiagnosticCode::StyleLowerSnake,
                format!("Field `{}` should use lower_snake_case", f.name.name),
                f.name.span,
            ));
        }
    }
    for o in &m.oneofs {
        if !is_lower_snake(&o.name.name) {
            out.push(warn(
                DiagnosticCode::StyleLowerSnake,
                format!("Oneof `{}` should use lower_snake_case", o.name.name),
                o.name.span,
            ));
        }
        for f in &o.fields {
            if !is_lower_snake(&f.name.name) {
                out.push(warn(
                    DiagnosticCode::StyleLowerSnake,
                    format!("Field `{}` should use lower_snake_case", f.name.name),
                    f.name.span,
                ));
            }
        }
    }
    for nm in &m.nested_messages {
        visit_message(nm, out);
    }
    for ne in &m.nested_enums {
        visit_enum(ne, out);
    }
}

fn visit_enum(e: &ast::EnumDecl, out: &mut Vec<ProtoDiagnostic>) {
    if !is_upper_camel(&e.name.name) {
        out.push(warn(
            DiagnosticCode::StyleUpperCamel,
            format!("Enum `{}` should use UpperCamelCase", e.name.name),
            e.name.span,
        ));
    }
    for v in &e.values {
        if !is_screaming_snake(&v.name.name) {
            out.push(warn(
                DiagnosticCode::StyleScreamingSnake,
                format!("Enum value `{}` should use SCREAMING_SNAKE_CASE", v.name.name),
                v.name.span,
            ));
        }
    }
}

fn visit_service(s: &ast::Service, out: &mut Vec<ProtoDiagnostic>) {
    if !is_upper_camel(&s.name.name) {
        out.push(warn(
            DiagnosticCode::StyleUpperCamel,
            format!("Service `{}` should use UpperCamelCase", s.name.name),
            s.name.span,
        ));
    }
    for m in &s.methods {
        if !is_upper_camel(&m.name.name) {
            out.push(warn(
                DiagnosticCode::StyleUpperCamel,
                format!("RPC `{}` should use UpperCamelCase", m.name.name),
                m.name.span,
            ));
        }
    }
}

fn is_upper_camel(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_uppercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric())
}

fn is_lower_snake(s: &str) -> bool {
    let mut saw_alpha = false;
    for c in s.chars() {
        if c.is_ascii_uppercase() {
            return false;
        }
        if c.is_ascii_lowercase() { saw_alpha = true; }
        if !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
            return false;
        }
    }
    saw_alpha
}

fn is_screaming_snake(s: &str) -> bool {
    let mut saw_alpha = false;
    for c in s.chars() {
        if c.is_ascii_lowercase() {
            return false;
        }
        if c.is_ascii_uppercase() { saw_alpha = true; }
        if !(c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') {
            return false;
        }
    }
    saw_alpha
}
