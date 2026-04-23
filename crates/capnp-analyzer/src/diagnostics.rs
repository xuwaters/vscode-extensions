//! Diagnostics over a parsed [`File`]. This first cut ships three checks:
//!
//! - Parse errors surfaced from the parser (`CAPNP0001`).
//! - Missing top-level file id (`CAPNP0002`) — every Cap'n Proto schema must
//!   open with `@0x…;`.
//! - Duplicate field / enumerant / method ordinals within the same scope
//!   (`CAPNP0010`).
//!
//! Name resolution (undefined type references, duplicate declarations,
//! import path validation) is deferred to a follow-up change that will also
//! maintain cross-file state in the VFS.

use crate::ast::*;
use crate::parser::{parse, ParseError};
use crate::spans::ByteSpan;
use rustc_hash::FxHashMap;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct CapnpDiagnostic {
    pub code: &'static str,
    pub severity: Severity,
    pub message: String,
    pub span: ByteSpan,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub enum Severity {
    Error,
    Warning,
}

pub struct Analysis {
    pub file: File,
    pub diagnostics: Vec<CapnpDiagnostic>,
}

pub fn analyze(source: &str) -> Analysis {
    let parsed = parse(source);
    let mut diagnostics: Vec<CapnpDiagnostic> = parsed
        .errors
        .into_iter()
        .map(parse_error_to_diag)
        .collect();

    if parsed.file.file_id.is_none() && !parsed.file.decls.is_empty() {
        // Report at the start of the file.
        let span = ByteSpan::new(0, 1.min(source.len() as u32));
        diagnostics.push(CapnpDiagnostic {
            code: "CAPNP0002",
            severity: Severity::Error,
            message: "missing file id: expected '@0x…;' at top of file".into(),
            span,
        });
    }

    check_decls(&parsed.file.decls, &mut diagnostics);

    Analysis { file: parsed.file, diagnostics }
}

fn parse_error_to_diag(e: ParseError) -> CapnpDiagnostic {
    CapnpDiagnostic {
        code: "CAPNP0001",
        severity: Severity::Error,
        message: e.message,
        span: e.span,
    }
}

fn check_decls(decls: &[Decl], out: &mut Vec<CapnpDiagnostic>) {
    for d in decls {
        match d {
            Decl::Struct(s) => check_struct(s, out),
            Decl::Enum(e) => check_enum(e, out),
            Decl::Interface(i) => check_interface(i, out),
            _ => {}
        }
    }
}

fn check_struct(s: &Struct, out: &mut Vec<CapnpDiagnostic>) {
    // Collect ordinals across fields, including fields nested in anonymous
    // unions (they share the struct's ordinal space).
    let mut seen: FxHashMap<u32, ByteSpan> = FxHashMap::default();
    for m in &s.members {
        match m {
            StructMember::Field(f) => {
                if let Some(ord) = &f.ordinal {
                    check_ordinal(ord, &mut seen, out);
                }
                if let FieldBody::NamedUnion(ub) = &f.body {
                    for inner in &ub.members {
                        if let Some(ord) = &inner.ordinal {
                            check_ordinal(ord, &mut seen, out);
                        }
                    }
                }
                if let FieldBody::NamedGroup(gb) = &f.body {
                    for inner in &gb.members {
                        if let StructMember::Field(f2) = inner {
                            if let Some(ord) = &f2.ordinal {
                                check_ordinal(ord, &mut seen, out);
                            }
                        }
                    }
                }
            }
            StructMember::AnonUnion(ub) => {
                for inner in &ub.members {
                    if let Some(ord) = &inner.ordinal {
                        check_ordinal(ord, &mut seen, out);
                    }
                }
            }
            _ => {}
        }
    }

    // Recurse into nested declarations.
    for m in &s.members {
        match m {
            StructMember::Struct(s2) => check_struct(s2, out),
            StructMember::Enum(e) => check_enum(e, out),
            StructMember::Interface(i) => check_interface(i, out),
            StructMember::Field(f) => {
                if let FieldBody::NamedGroup(gb) = &f.body {
                    // Treat groups as their own sub-scope too? In capnp
                    // groups share the parent's ordinal space, so the pass
                    // above already accounted for them.
                    for inner in &gb.members {
                        match inner {
                            StructMember::Struct(s2) => check_struct(s2, out),
                            StructMember::Enum(e) => check_enum(e, out),
                            StructMember::Interface(i) => check_interface(i, out),
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn check_enum(e: &EnumDecl, out: &mut Vec<CapnpDiagnostic>) {
    let mut seen: FxHashMap<u32, ByteSpan> = FxHashMap::default();
    for en in &e.enumerants {
        if let Some(ord) = &en.ordinal {
            check_ordinal(ord, &mut seen, out);
        }
    }
}

fn check_interface(i: &Interface, out: &mut Vec<CapnpDiagnostic>) {
    let mut seen: FxHashMap<u32, ByteSpan> = FxHashMap::default();
    for m in &i.methods {
        if let Some(ord) = &m.ordinal {
            check_ordinal(ord, &mut seen, out);
        }
    }
    for n in &i.nested {
        match n {
            StructMember::Struct(s2) => check_struct(s2, out),
            StructMember::Enum(e) => check_enum(e, out),
            StructMember::Interface(i2) => check_interface(i2, out),
            _ => {}
        }
    }
}

fn check_ordinal(
    ord: &Ordinal,
    seen: &mut FxHashMap<u32, ByteSpan>,
    out: &mut Vec<CapnpDiagnostic>,
) {
    if let Some(prev) = seen.get(&ord.value).copied() {
        out.push(CapnpDiagnostic {
            code: "CAPNP0010",
            severity: Severity::Error,
            message: format!(
                "ordinal @{} already used (first seen at bytes {}..{})",
                ord.value, prev.start, prev.end
            ),
            span: ord.span,
        });
    } else {
        seen.insert(ord.value, ord.span);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_missing_file_id() {
        let a = analyze("struct X { id @0 :UInt32; }");
        assert!(a.diagnostics.iter().any(|d| d.code == "CAPNP0002"));
    }

    #[test]
    fn reports_duplicate_ordinal() {
        let a = analyze("@0x1; struct X { a @0 :UInt32; b @0 :Text; }");
        assert!(a.diagnostics.iter().any(|d| d.code == "CAPNP0010"));
    }

    #[test]
    fn clean_file_has_no_diagnostics() {
        let a = analyze("@0x1; struct X { a @0 :UInt32; b @1 :Text; }");
        assert!(a.diagnostics.is_empty(), "{:?}", a.diagnostics);
    }
}
