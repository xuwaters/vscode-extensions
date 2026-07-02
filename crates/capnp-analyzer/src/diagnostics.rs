//! Diagnostics.
//!
//! Two entry points:
//!
//! - [`analyze`] runs the per-file checks during parse: parse errors
//!   (`CAPNP0001`), missing top-level file id (`CAPNP0002`), duplicate
//!   field / enumerant / method ordinals within the same scope
//!   (`CAPNP0010`). It does not require a workspace and is safe to call
//!   during `update_file`.
//! - [`workspace_diagnostics`] adds cross-file checks that need a built
//!   [`WorkspaceIndex`]: unresolved `import "…"` paths (`CAPNP0020`) and
//!   unresolved type references (`CAPNP0030`).

use crate::ast::*;
use crate::parser::{parse, ParseError};
use crate::resolve::{collect_type_use_sites, is_builtin, Resolution, WorkspaceIndex};
use crate::spans::ByteSpan;
use crate::vfs::{FileUri, Workspace};
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

/// Workspace-level diagnostics that need a resolved symbol index:
/// unresolved imports and unresolved type references.
pub fn workspace_diagnostics(
    ws: &Workspace,
    index: &WorkspaceIndex,
    uri: &FileUri,
) -> Vec<CapnpDiagnostic> {
    let Some(state) = ws.file(uri) else { return Vec::new() };
    let mut out = Vec::new();

    // Unresolved imports (`using X = import "…";`).
    for d in &state.analysis.file.decls {
        if let Decl::Using(u) = d {
            if let Some(path) = &u.import_path {
                if ws.resolve_import_path(uri, &path.value).is_none() {
                    out.push(CapnpDiagnostic {
                        code: "CAPNP0020",
                        severity: Severity::Warning,
                        message: format!("cannot resolve import \"{}\"", path.value),
                        span: path.span,
                    });
                }
            }
        }
    }

    // Unresolved type references.
    for site in collect_type_use_sites(&state.analysis.file) {
        if site.import_path.is_none() && site.path.len() == 1 && is_builtin(&site.path[0].text) {
            continue;
        }
        if let Some(import_path) = &site.import_path {
            if ws.resolve_import_path(uri, import_path).is_none() {
                out.push(CapnpDiagnostic {
                    code: "CAPNP0020",
                    severity: Severity::Warning,
                    message: format!("cannot resolve import \"{}\"", import_path),
                    span: site.span,
                });
                continue;
            }
        }
        match index.resolve_type_with_import(
            uri,
            site.enclosing_scope.as_str(),
            site.import_path.as_deref(),
            &site.path,
        ) {
            Resolution::Found { visibility_ok: false, symbol } => {
                out.push(CapnpDiagnostic {
                    code: "CAPNP0031",
                    severity: Severity::Warning,
                    message: format!(
                        "type `{}` is defined in {} but that file is not imported here",
                        symbol.fqn,
                        symbol.file.as_str(),
                    ),
                    span: site.span,
                });
            }
            Resolution::Unknown { .. } => {
                let shown = site
                    .path
                    .iter()
                    .map(|i| i.text.as_str())
                    .collect::<Vec<_>>()
                    .join(".");
                out.push(CapnpDiagnostic {
                    code: "CAPNP0030",
                    severity: Severity::Warning,
                    message: format!("unknown type `{}`", shown),
                    span: site.span,
                });
            }
            _ => {}
        }
    }

    out
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

    #[test]
    fn workspace_reports_unresolved_type() {
        use crate::resolve::WorkspaceIndex;
        use crate::vfs::{FileUri, Workspace};
        let mut ws = Workspace::new();
        ws.update(
            "file:///a.capnp",
            "@0x1; struct S { f @0 :DoesNotExist; }".into(),
        );
        let idx = WorkspaceIndex::build(&ws);
        let diags = workspace_diagnostics(&ws, &idx, &FileUri("file:///a.capnp".into()));
        assert!(diags.iter().any(|d| d.code == "CAPNP0030"));
    }

    #[test]
    fn local_alias_to_builtin_is_not_unknown() {
        use crate::resolve::WorkspaceIndex;
        use crate::vfs::{FileUri, Workspace};
        let mut ws = Workspace::new();
        ws.update(
            "file:///a.capnp",
            "@0x1; using SlotId = Int16; struct S { slot @0 :SlotId; }".into(),
        );
        let idx = WorkspaceIndex::build(&ws);
        let diags = workspace_diagnostics(&ws, &idx, &FileUri("file:///a.capnp".into()));
        assert!(diags.is_empty(), "{:?}", diags);
    }

    #[test]
    fn local_alias_to_local_type_is_not_unknown() {
        use crate::resolve::WorkspaceIndex;
        use crate::vfs::{FileUri, Workspace};
        let mut ws = Workspace::new();
        ws.update(
            "file:///a.capnp",
            "@0x1; struct Point { x @0 :Int16; } using P = Point; struct S { p @0 :P; }".into(),
        );
        let idx = WorkspaceIndex::build(&ws);
        let diags = workspace_diagnostics(&ws, &idx, &FileUri("file:///a.capnp".into()));
        assert!(diags.is_empty(), "{:?}", diags);
    }

    #[test]
    fn local_alias_to_unknown_type_still_reports() {
        use crate::resolve::WorkspaceIndex;
        use crate::vfs::{FileUri, Workspace};
        let mut ws = Workspace::new();
        ws.update(
            "file:///a.capnp",
            "@0x1; using B = Missing; struct S { b @0 :B; }".into(),
        );
        let idx = WorkspaceIndex::build(&ws);
        let diags = workspace_diagnostics(&ws, &idx, &FileUri("file:///a.capnp".into()));
        assert!(diags.iter().any(|d| d.code == "CAPNP0030"), "{:?}", diags);
    }

    #[test]
    fn local_alias_cycle_terminates_and_reports() {
        use crate::resolve::WorkspaceIndex;
        use crate::vfs::{FileUri, Workspace};
        let mut ws = Workspace::new();
        ws.update(
            "file:///a.capnp",
            "@0x1; using A = B; using B = A; struct S { x @0 :A; }".into(),
        );
        let idx = WorkspaceIndex::build(&ws);
        let diags = workspace_diagnostics(&ws, &idx, &FileUri("file:///a.capnp".into()));
        assert!(diags.iter().any(|d| d.code == "CAPNP0030"), "{:?}", diags);
    }

    #[test]
    fn workspace_reports_unresolved_import() {
        use crate::resolve::WorkspaceIndex;
        use crate::vfs::{FileUri, Workspace};
        let mut ws = Workspace::new();
        ws.update(
            "file:///a.capnp",
            "@0x1; using X = import \"missing.capnp\";".into(),
        );
        let idx = WorkspaceIndex::build(&ws);
        let diags = workspace_diagnostics(&ws, &idx, &FileUri("file:///a.capnp".into()));
        assert!(diags.iter().any(|d| d.code == "CAPNP0020"));
    }

    #[test]
    fn workspace_reports_unresolved_inline_import() {
        use crate::resolve::WorkspaceIndex;
        use crate::vfs::{FileUri, Workspace};
        let mut ws = Workspace::new();
        ws.update(
            "file:///a.capnp",
            "@0x1; struct S { f @0 :import \"missing.capnp\".Foo; }".into(),
        );
        let idx = WorkspaceIndex::build(&ws);
        let diags = workspace_diagnostics(&ws, &idx, &FileUri("file:///a.capnp".into()));
        assert!(diags.iter().any(|d| d.code == "CAPNP0020"));
    }
}
