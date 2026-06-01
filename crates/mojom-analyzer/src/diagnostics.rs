//! Diagnostics.
//!
//! Two entry points:
//!
//! - [`analyze`] runs the per-file checks during parse: parse errors
//!   (`MOJOM0001`) and duplicate ordinals within the same scope
//!   (`MOJOM0010`). It needs no workspace and is safe to call during
//!   `update_file`.
//! - [`workspace_diagnostics`] adds cross-file checks that need a built
//!   [`WorkspaceIndex`]: unresolved `import "…"` paths (`MOJOM0020`),
//!   unknown type references (`MOJOM0030`), and references to a type that
//!   exists but lives in a file that isn't imported here (`MOJOM0031`).

use crate::ast::*;
use crate::parser::{parse, ParseError};
use crate::resolve::{collect_type_use_sites, Resolution, WorkspaceIndex};
use crate::spans::ByteSpan;
use crate::vfs::{FileUri, Workspace};
use rustc_hash::FxHashMap;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct MojomDiagnostic {
    pub code: &'static str,
    pub severity: Severity,
    pub message: String,
    pub span: ByteSpan,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

pub struct Analysis {
    pub file: File,
    pub diagnostics: Vec<MojomDiagnostic>,
}

pub fn analyze(source: &str) -> Analysis {
    let parsed = parse(source);
    let mut diagnostics: Vec<MojomDiagnostic> =
        parsed.errors.into_iter().map(parse_error_to_diag).collect();

    check_decls(&parsed.file.decls, &mut diagnostics);

    Analysis { file: parsed.file, diagnostics }
}

fn parse_error_to_diag(e: ParseError) -> MojomDiagnostic {
    MojomDiagnostic {
        code: "MOJOM0001",
        severity: Severity::Error,
        message: e.message,
        span: e.span,
    }
}

fn check_decls(decls: &[Decl], out: &mut Vec<MojomDiagnostic>) {
    for d in decls {
        match d {
            Decl::Struct(s) => check_struct(s, out),
            Decl::Union(u) => check_union(u, out),
            Decl::Interface(i) => check_interface(i, out),
            _ => {}
        }
    }
}

fn check_struct(s: &Struct, out: &mut Vec<MojomDiagnostic>) {
    let mut seen: FxHashMap<u32, ByteSpan> = FxHashMap::default();
    for m in &s.members {
        match m {
            StructMember::Field(f) => {
                if let Some(ord) = &f.ordinal {
                    check_ordinal(ord, &mut seen, out);
                }
            }
            StructMember::Enum(e) => check_enum(e, out),
            StructMember::Const(_) => {}
        }
    }
}

fn check_union(u: &Union, out: &mut Vec<MojomDiagnostic>) {
    let mut seen: FxHashMap<u32, ByteSpan> = FxHashMap::default();
    for f in &u.fields {
        if let Some(ord) = &f.ordinal {
            check_ordinal(ord, &mut seen, out);
        }
    }
}

fn check_interface(i: &Interface, out: &mut Vec<MojomDiagnostic>) {
    let mut method_ords: FxHashMap<u32, ByteSpan> = FxHashMap::default();
    for m in &i.members {
        match m {
            InterfaceMember::Method(meth) => {
                if let Some(ord) = &meth.ordinal {
                    check_ordinal(ord, &mut method_ords, out);
                }
                let mut param_ords: FxHashMap<u32, ByteSpan> = FxHashMap::default();
                for p in &meth.params {
                    if let Some(ord) = &p.ordinal {
                        check_ordinal(ord, &mut param_ords, out);
                    }
                }
                if let Some(resp) = &meth.response {
                    let mut resp_ords: FxHashMap<u32, ByteSpan> = FxHashMap::default();
                    for p in resp {
                        if let Some(ord) = &p.ordinal {
                            check_ordinal(ord, &mut resp_ords, out);
                        }
                    }
                }
            }
            InterfaceMember::Enum(e) => check_enum(e, out),
            InterfaceMember::Const(_) => {}
        }
    }
}

fn check_enum(_e: &EnumDecl, _out: &mut [MojomDiagnostic]) {
    // Enum values use `= <expr>` initialisers rather than `@` ordinals, so
    // there is nothing ordinal-shaped to validate here yet.
}

fn check_ordinal(ord: &Ordinal, seen: &mut FxHashMap<u32, ByteSpan>, out: &mut Vec<MojomDiagnostic>) {
    if let Some(prev) = seen.get(&ord.value).copied() {
        out.push(MojomDiagnostic {
            code: "MOJOM0010",
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

/// Workspace-level diagnostics that need a resolved symbol index: unresolved
/// imports and unknown / not-imported type references.
pub fn workspace_diagnostics(
    ws: &Workspace,
    index: &WorkspaceIndex,
    uri: &FileUri,
) -> Vec<MojomDiagnostic> {
    let Some(state) = ws.file(uri) else { return Vec::new() };
    let mut out = Vec::new();

    for imp in &state.analysis.file.imports {
        if ws.resolve_import_path(uri, &imp.path.value).is_none() {
            out.push(MojomDiagnostic {
                code: "MOJOM0020",
                severity: Severity::Warning,
                message: format!("cannot resolve import \"{}\"", imp.path.value),
                span: imp.path.span,
            });
        }
    }

    for site in collect_type_use_sites(&state.analysis.file) {
        match index.resolve_type(uri, site.enclosing_scope.as_str(), &site.path) {
            Resolution::Found { visibility_ok: false, symbol } => {
                out.push(MojomDiagnostic {
                    code: "MOJOM0031",
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
                let shown = site.path.iter().map(|i| i.text.as_str()).collect::<Vec<_>>().join(".");
                out.push(MojomDiagnostic {
                    code: "MOJOM0030",
                    severity: Severity::Warning,
                    message: format!("unknown type `{}`", shown),
                    span: site.span,
                });
            }
            Resolution::Found { visibility_ok: true, .. } => {}
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_duplicate_field_ordinal() {
        let a = analyze("struct S { int32 a@0; int32 b@0; };");
        assert!(a.diagnostics.iter().any(|d| d.code == "MOJOM0010"));
    }

    #[test]
    fn reports_duplicate_method_ordinal() {
        let a = analyze("interface I { A@1(); B@1(); };");
        assert!(a.diagnostics.iter().any(|d| d.code == "MOJOM0010"));
    }

    #[test]
    fn clean_file_has_no_diagnostics() {
        let a = analyze("module m;\nstruct S { int32 a@0; int32 b@1; };");
        assert!(a.diagnostics.is_empty(), "{:?}", a.diagnostics);
    }

    #[test]
    fn workspace_reports_unresolved_import() {
        let mut ws = Workspace::new();
        ws.update("file:///a.mojom", "import \"missing.mojom\";".into());
        let idx = WorkspaceIndex::build(&ws);
        let diags = workspace_diagnostics(&ws, &idx, &FileUri("file:///a.mojom".into()));
        assert!(diags.iter().any(|d| d.code == "MOJOM0020"));
    }

    #[test]
    fn workspace_reports_unknown_type() {
        let mut ws = Workspace::new();
        ws.update("file:///a.mojom", "struct S { DoesNotExist f; };".into());
        let idx = WorkspaceIndex::build(&ws);
        let diags = workspace_diagnostics(&ws, &idx, &FileUri("file:///a.mojom".into()));
        assert!(diags.iter().any(|d| d.code == "MOJOM0030"));
    }

    #[test]
    fn builtin_types_are_not_unknown() {
        let mut ws = Workspace::new();
        ws.update(
            "file:///a.mojom",
            "struct S { int32 a; string b; array<uint8> c; map<string, int64> d; };".into(),
        );
        let idx = WorkspaceIndex::build(&ws);
        let diags = workspace_diagnostics(&ws, &idx, &FileUri("file:///a.mojom".into()));
        assert!(diags.is_empty(), "{:?}", diags);
    }
}
