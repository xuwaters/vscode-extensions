//! Diagnostic producers that need cross-file context: unresolved types,
//! unused imports, circular imports.

use super::{DiagnosticCode, ProtoDiagnostic, Severity};
use crate::ast;
use crate::resolve::{collect_type_use_sites, Resolution, WorkspaceIndex};
use crate::vfs::{FileUri, Workspace};
use rustc_hash::{FxHashMap, FxHashSet};

pub fn run_resolve_checks(
    ws: &Workspace,
    index: &WorkspaceIndex,
    uri: &FileUri,
) -> Vec<ProtoDiagnostic> {
    let Some(pf) = ws.file(uri) else { return Vec::new() };
    let mut out = Vec::new();

    let use_sites = collect_type_use_sites(&pf.ast);

    // Track which imports were actually consumed (by FQN -> file URI).
    let mut used_files: FxHashSet<FileUri> = FxHashSet::default();

    for site in &use_sites {
        match index.resolve_type(uri, site.enclosing_scope.as_str(), &site.name) {
            Resolution::Found { symbol, visibility_ok } => {
                used_files.insert(symbol.file.clone());
                if !symbol.kind.is_type() {
                    out.push(ProtoDiagnostic::new(
                        DiagnosticCode::NotAType,
                        Severity::Error,
                        format!(
                            "Cannot use `{}` as a type (it is a {:?})",
                            site.name.to_display(),
                            symbol.kind
                        ),
                        site.span,
                    ));
                } else if !visibility_ok {
                    let suggestion = relative_import_path(&symbol.file);
                    out.push(ProtoDiagnostic::new(
                        DiagnosticCode::UnknownType,
                        Severity::Error,
                        format!(
                            "Type `{}` is defined in `{}` but not imported here{}",
                            site.name.to_display(),
                            symbol.file.as_str(),
                            suggestion
                                .map(|s| format!(" (add `import \"{}\";`)", s))
                                .unwrap_or_default(),
                        ),
                        site.span,
                    ));
                }
            }
            Resolution::Unknown { .. } => {
                let hint = index
                    .suggest_similar(&site.name.to_display())
                    .into_iter()
                    .next();
                out.push(ProtoDiagnostic::new(
                    DiagnosticCode::UnknownType,
                    Severity::Error,
                    match hint {
                        Some(h) => format!(
                            "Unknown type `{}` (did you mean `{}`?)",
                            site.name.to_display(),
                            h
                        ),
                        None => format!("Unknown type `{}`", site.name.to_display()),
                    },
                    site.span,
                ));
            }
        }
    }

    // Unused imports: any imported file that didn't contribute a resolution.
    for imp in &pf.ast.imports {
        if matches!(imp.modifier, ast::ImportModifier::Weak) {
            continue;
        }
        let Some(target) = ws.resolve_import_path(uri, &imp.path) else {
            continue; // ImportUnresolved is reported elsewhere.
        };
        if matches!(imp.modifier, ast::ImportModifier::Public) {
            // `public import` is effectively re-exporting; usage is satisfied
            // by downstream importers.
            continue;
        }
        // The `descriptor.proto` / well-known-types imports that feed
        // options (e.g. `import "google/protobuf/descriptor.proto";` for
        // extensions) don't show up in type use-sites — skip WKT imports in
        // the unused check to avoid false positives on option-only usage.
        if target.as_str().starts_with("proto3-wkt:") {
            continue;
        }
        if !used_files.contains(&target) {
            out.push(ProtoDiagnostic::new(
                DiagnosticCode::ImportUnused,
                Severity::Warning,
                format!("Import `\"{}\"` is never used", imp.path),
                imp.path_span,
            ));
        }
    }

    // Circular-import detection: BFS through direct imports of `uri` looking
    // for a cycle back to `uri`.
    if let Some(cycle) = find_cycle(ws, uri) {
        let a = uri.as_str().rsplit('/').next().unwrap_or(uri.as_str());
        let b = cycle.as_str().rsplit('/').next().unwrap_or(cycle.as_str());
        // Attribute the warning to the first import that participates in
        // the cycle.
        if let Some(imp) = pf.ast.imports.iter().find(|imp| {
            ws.resolve_import_path(uri, &imp.path).is_some_and(|t| t == cycle)
        }) {
            out.push(ProtoDiagnostic::new(
                DiagnosticCode::ImportCircular,
                Severity::Warning,
                format!("Circular import: `{}` ⇄ `{}`", a, b),
                imp.path_span,
            ));
        }
    }

    out
}

fn find_cycle(ws: &Workspace, start: &FileUri) -> Option<FileUri> {
    let mut stack = vec![start.clone()];
    let mut parent: FxHashMap<FileUri, FileUri> = FxHashMap::default();
    let mut seen: FxHashSet<FileUri> = FxHashSet::default();
    seen.insert(start.clone());
    while let Some(u) = stack.pop() {
        let Some(pf) = ws.file(&u) else { continue };
        for imp in &pf.ast.imports {
            let Some(target) = ws.resolve_import_path(&u, &imp.path) else { continue };
            if &target == start {
                return Some(u);
            }
            if seen.insert(target.clone()) {
                parent.insert(target.clone(), u.clone());
                stack.push(target);
            }
        }
    }
    None
}

fn relative_import_path(uri: &FileUri) -> Option<String> {
    // Best-effort: strip the leading `proto3-wkt:/` or leave the raw tail.
    let s = uri.as_str();
    if let Some(rest) = s.strip_prefix("proto3-wkt:/") {
        return Some(rest.to_string());
    }
    if let Some((_, tail)) = s.rsplit_once("://") {
        return Some(tail.to_string());
    }
    None
}
