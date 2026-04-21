//! Semantic-diagnostic producers that run after parsing. Phase-1 set:
//! duplicate names, duplicate / out-of-range / reserved field numbers,
//! enum first-value-must-be-zero, oneof invariants, map key type.

use super::{DiagnosticCode, ProtoDiagnostic, Severity};
use crate::ast;
use crate::spans::ByteSpan;
use rustc_hash::FxHashMap;

const MAX_FIELD_NUMBER: i64 = 536_870_911;
const RESERVED_RANGE_START: i64 = 19_000;
const RESERVED_RANGE_END: i64 = 19_999;

pub fn run_all_checks(file: &ast::File) -> Vec<ProtoDiagnostic> {
    let mut out = Vec::new();
    check_top_level_duplicates(file, &mut out);
    for item in &file.items {
        match item {
            ast::TopLevelItem::Message(m) => check_message(m, &mut out),
            ast::TopLevelItem::Enum(e) => check_enum(e, &mut out),
            ast::TopLevelItem::Service(s) => check_service(s, &mut out),
            ast::TopLevelItem::Extend(_) => {}
        }
    }
    out
}

fn check_top_level_duplicates(file: &ast::File, out: &mut Vec<ProtoDiagnostic>) {
    let mut seen: FxHashMap<&str, ByteSpan> = FxHashMap::default();
    for item in &file.items {
        let (name, span) = match item {
            ast::TopLevelItem::Message(m) => (m.name.name.as_str(), m.name.span),
            ast::TopLevelItem::Enum(e) => (e.name.name.as_str(), e.name.span),
            ast::TopLevelItem::Service(s) => (s.name.name.as_str(), s.name.span),
            ast::TopLevelItem::Extend(_) => continue,
        };
        if let Some(_prev) = seen.insert(name, span) {
            out.push(ProtoDiagnostic::new(
                DiagnosticCode::DuplicateName,
                Severity::Error,
                format!("Duplicate top-level name `{}`", name),
                span,
            ));
        }
    }
}

fn check_message(m: &ast::Message, out: &mut Vec<ProtoDiagnostic>) {
    // Gather all fields (own + oneof) for number/name uniqueness.
    let mut field_numbers: FxHashMap<i64, ByteSpan> = FxHashMap::default();
    let mut field_names: FxHashMap<String, ByteSpan> = FxHashMap::default();

    for f in m.fields.iter().chain(m.oneofs.iter().flat_map(|o| o.fields.iter())) {
        check_field_number(m, f, &mut field_numbers, out);
        check_field_name(m, f, &mut field_names, out);
        check_field_map(f, out);
    }

    for o in &m.oneofs {
        for f in &o.fields {
            match f.label {
                ast::FieldLabel::Repeated => {
                    out.push(ProtoDiagnostic::new(
                        DiagnosticCode::OneofRepeatedField,
                        Severity::Error,
                        format!("Oneof `{}` cannot contain `repeated` field `{}`", o.name.name, f.name.name),
                        f.span,
                    ));
                }
                _ => {}
            }
            if matches!(&f.ty, ast::TypeRef::Map(_)) {
                out.push(ProtoDiagnostic::new(
                    DiagnosticCode::OneofMapField,
                    Severity::Error,
                    format!("Oneof `{}` cannot contain `map` field `{}`", o.name.name, f.name.name),
                    f.span,
                ));
            }
        }
    }

    // Nested duplicates (messages / enums)
    let mut nested_names: FxHashMap<String, ByteSpan> = FxHashMap::default();
    for nm in &m.nested_messages {
        if nested_names.insert(nm.name.name.to_string(), nm.name.span).is_some() {
            out.push(ProtoDiagnostic::new(
                DiagnosticCode::DuplicateName,
                Severity::Error,
                format!("Duplicate nested name `{}` in `{}`", nm.name.name, m.name.name),
                nm.name.span,
            ));
        }
        check_message(nm, out);
    }
    for ne in &m.nested_enums {
        if nested_names.insert(ne.name.name.to_string(), ne.name.span).is_some() {
            out.push(ProtoDiagnostic::new(
                DiagnosticCode::DuplicateName,
                Severity::Error,
                format!("Duplicate nested name `{}` in `{}`", ne.name.name, m.name.name),
                ne.name.span,
            ));
        }
        check_enum(ne, out);
    }
}

fn check_field_number(
    m: &ast::Message,
    f: &ast::FieldDecl,
    seen: &mut FxHashMap<i64, ByteSpan>,
    out: &mut Vec<ProtoDiagnostic>,
) {
    let Some(n) = f.number.as_i64() else { return };
    if !(1..=MAX_FIELD_NUMBER).contains(&n) {
        out.push(ProtoDiagnostic::new(
            DiagnosticCode::FieldNumberOutOfRange,
            Severity::Error,
            format!("Field number {} is outside the valid range 1..={}", n, MAX_FIELD_NUMBER),
            f.number.span,
        ));
        return;
    }
    if (RESERVED_RANGE_START..=RESERVED_RANGE_END).contains(&n) {
        out.push(ProtoDiagnostic::new(
            DiagnosticCode::FieldNumberReservedRange,
            Severity::Error,
            format!("Field number {} is in the reserved range {}..={}", n, RESERVED_RANGE_START, RESERVED_RANGE_END),
            f.number.span,
        ));
    }
    for r in &m.reserved {
        for item in &r.items {
            let hit = match item {
                ast::ReservedItem::Number(v) => v.as_i64().map_or(false, |rv| rv == n),
                ast::ReservedItem::Range { from, to, .. } => {
                    let lo = from.as_i64().unwrap_or(n);
                    let hi = match to {
                        ast::ReservedRangeEnd::Value(v) => v.as_i64().unwrap_or(n),
                        ast::ReservedRangeEnd::Max(_) => MAX_FIELD_NUMBER,
                    };
                    lo <= n && n <= hi
                }
                _ => false,
            };
            if hit {
                out.push(ProtoDiagnostic::new(
                    DiagnosticCode::FieldNumberReserved,
                    Severity::Error,
                    format!("Field number {} conflicts with `reserved` in `{}`", n, m.name.name),
                    f.number.span,
                ));
            }
        }
    }
    if let Some(_prev) = seen.insert(n, f.number.span) {
        out.push(ProtoDiagnostic::new(
            DiagnosticCode::DuplicateFieldNumber,
            Severity::Error,
            format!("Duplicate field number {} in `{}`", n, m.name.name),
            f.number.span,
        ));
    }
}

fn check_field_name(
    m: &ast::Message,
    f: &ast::FieldDecl,
    seen: &mut FxHashMap<String, ByteSpan>,
    out: &mut Vec<ProtoDiagnostic>,
) {
    let key = f.name.name.to_string();
    if let Some(_prev) = seen.insert(key.clone(), f.name.span) {
        out.push(ProtoDiagnostic::new(
            DiagnosticCode::DuplicateName,
            Severity::Error,
            format!("Duplicate field name `{}` in `{}`", f.name.name, m.name.name),
            f.name.span,
        ));
    }
    for r in &m.reserved {
        for item in &r.items {
            if let ast::ReservedItem::Name(n, _) = item {
                if n == key.as_str() {
                    out.push(ProtoDiagnostic::new(
                        DiagnosticCode::FieldNameReserved,
                        Severity::Error,
                        format!("Field name `{}` conflicts with `reserved` name in `{}`", key, m.name.name),
                        f.name.span,
                    ));
                }
            }
        }
    }
}

fn check_field_map(f: &ast::FieldDecl, out: &mut Vec<ProtoDiagnostic>) {
    let ast::TypeRef::Map(m) = &f.ty else { return };
    match &m.key {
        ast::TypeRef::Scalar(s, sp) => {
            if !s.is_valid_map_key() {
                out.push(ProtoDiagnostic::new(
                    DiagnosticCode::MapKeyTypeInvalid,
                    Severity::Error,
                    format!("map<{}, …> key type must be an integral or string type", s.as_str()),
                    *sp,
                ));
            }
        }
        ast::TypeRef::Named(q) => {
            out.push(ProtoDiagnostic::new(
                DiagnosticCode::MapKeyTypeInvalid,
                Severity::Error,
                format!("map<{}, …> key type must be an integral or string type", q.to_display()),
                q.span,
            ));
        }
        ast::TypeRef::Map(inner) => {
            out.push(ProtoDiagnostic::new(
                DiagnosticCode::MapKeyTypeInvalid,
                Severity::Error,
                "map key cannot be a map".into(),
                inner.span,
            ));
        }
        ast::TypeRef::Missing(_) => {}
    }
}

fn check_enum(e: &ast::EnumDecl, out: &mut Vec<ProtoDiagnostic>) {
    let mut names: FxHashMap<String, ByteSpan> = FxHashMap::default();
    for v in &e.values {
        if names.insert(v.name.name.to_string(), v.name.span).is_some() {
            out.push(ProtoDiagnostic::new(
                DiagnosticCode::DuplicateEnumValue,
                Severity::Error,
                format!("Duplicate enum value `{}` in `{}`", v.name.name, e.name.name),
                v.name.span,
            ));
        }
    }
    if let Some(first) = e.values.first() {
        if first.number.as_i64().unwrap_or(-1) != 0 {
            out.push(ProtoDiagnostic::new(
                DiagnosticCode::Proto3EnumFirstValueZero,
                Severity::Error,
                format!("Enum `{}` first value must be 0 in proto3", e.name.name),
                first.number.span,
            ));
        }
    }
}

fn check_service(s: &ast::Service, out: &mut Vec<ProtoDiagnostic>) {
    let mut names: FxHashMap<String, ByteSpan> = FxHashMap::default();
    for m in &s.methods {
        if names.insert(m.name.name.to_string(), m.name.span).is_some() {
            out.push(ProtoDiagnostic::new(
                DiagnosticCode::DuplicateName,
                Severity::Error,
                format!("Duplicate rpc name `{}` in service `{}`", m.name.name, s.name.name),
                m.name.span,
            ));
        }
    }
}
