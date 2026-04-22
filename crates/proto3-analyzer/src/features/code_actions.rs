//! Quick-fix code actions.
//!
//! - **Add missing import** — when an unknown-type diagnostic points at a
//!   symbol the workspace index *does* know about, offer an insert edit at
//!   the right spot in the import block.
//! - **Organize imports** — sort imports alphabetically (with modifier
//!   rank) and drop duplicates / imports unused elsewhere in the file.

use crate::ast;
use crate::features::position::type_use_at;
use crate::resolve::{Resolution, WorkspaceIndex};
use crate::spans::ByteSpan;
use crate::vfs::{FileUri, Workspace};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeAction {
    pub title: String,
    pub kind: String,
    pub edits: Vec<CodeEdit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeEdit {
    pub file: String,
    pub range: ByteSpan,
    pub new_text: String,
}

/// Code actions available at `offset` in `uri`, filtered by
/// `diagnostic_codes` if supplied (empty vector = no filtering).
pub fn code_actions(
    ws: &Workspace,
    index: &WorkspaceIndex,
    uri: &FileUri,
    offset: u32,
    _diagnostic_codes: &[String],
) -> Vec<CodeAction> {
    let mut out = Vec::new();

    if let Some(action) = add_missing_import(ws, index, uri, offset) {
        out.push(action);
    }
    if let Some(action) = organize_imports(ws, uri) {
        out.push(action);
    }
    out
}

fn add_missing_import(
    ws: &Workspace,
    index: &WorkspaceIndex,
    uri: &FileUri,
    offset: u32,
) -> Option<CodeAction> {
    let pf = ws.file(uri)?;
    let site = type_use_at(&pf.ast, offset)?;
    // Unresolved — or resolved but not visible.
    let (target_file, visibility_ok) =
        match index.resolve_type(uri, site.enclosing_scope.as_str(), &site.name) {
            Resolution::Found { symbol, visibility_ok } => (symbol.file.clone(), visibility_ok),
            Resolution::Unknown { .. } => return None,
        };
    if visibility_ok {
        return None;
    }

    let import_path = import_path_for(&target_file)?;

    // Already imported? (Can happen when visibility is blocked for another
    // reason — e.g. public-import chain.)
    for imp in &pf.ast.imports {
        if imp.path == import_path {
            return None;
        }
    }

    let (range, new_text) = import_insertion(pf, &import_path);

    Some(CodeAction {
        title: format!("Add import \"{}\"", import_path),
        kind: "quickfix".into(),
        edits: vec![CodeEdit { file: uri.as_str().to_string(), range, new_text }],
    })
}

fn organize_imports(ws: &Workspace, uri: &FileUri) -> Option<CodeAction> {
    let pf = ws.file(uri)?;
    if pf.ast.imports.len() < 2 {
        return None;
    }

    let mut imports = pf.ast.imports.clone();
    imports.sort_by(|a, b| {
        fn rank(m: ast::ImportModifier) -> u8 {
            match m {
                ast::ImportModifier::None => 0,
                ast::ImportModifier::Public => 1,
                ast::ImportModifier::Weak => 2,
            }
        }
        rank(a.modifier)
            .cmp(&rank(b.modifier))
            .then_with(|| a.path.cmp(&b.path))
    });
    imports.dedup_by(|a, b| a.path == b.path && a.modifier as u8 == b.modifier as u8);

    // If the order is already canonical, don't offer the action.
    let already_canonical = pf
        .ast
        .imports
        .iter()
        .zip(imports.iter())
        .all(|(a, b)| a.path == b.path && a.modifier as u8 == b.modifier as u8);
    if already_canonical && imports.len() == pf.ast.imports.len() {
        return None;
    }

    let first = pf.ast.imports.first()?;
    let last = pf.ast.imports.last()?;
    let range = ByteSpan::new(first.span.start, last.span.end);

    let new_text = imports
        .iter()
        .map(|imp| {
            let modifier = match imp.modifier {
                ast::ImportModifier::None => "",
                ast::ImportModifier::Public => "public ",
                ast::ImportModifier::Weak => "weak ",
            };
            format!("import {}\"{}\";", modifier, imp.path)
        })
        .collect::<Vec<_>>()
        .join("\n");

    Some(CodeAction {
        title: "Organize imports".into(),
        kind: "source.organizeImports".into(),
        edits: vec![CodeEdit { file: uri.as_str().to_string(), range, new_text }],
    })
}

fn import_path_for(uri: &FileUri) -> Option<String> {
    let s = uri.as_str();
    if let Some(rest) = s.strip_prefix("proto3-wkt:/") {
        return Some(rest.to_string());
    }
    if let Some((_, tail)) = s.rsplit_once("://") {
        return Some(tail.to_string());
    }
    if let Some(tail) = s.strip_prefix("file://") {
        return Some(tail.to_string());
    }
    Some(s.to_string())
}

fn import_insertion(pf: &crate::parse::ParsedFile, path: &str) -> (ByteSpan, String) {
    // Insert after the last import if any exists, alphabetically sorted
    // relative to existing imports of the same modifier.
    if !pf.ast.imports.is_empty() {
        // Find the insertion position among None-modifier imports.
        let mut insert_after: Option<&ast::Import> = None;
        for imp in &pf.ast.imports {
            if !matches!(imp.modifier, ast::ImportModifier::None) {
                continue;
            }
            if imp.path.as_str() < path {
                insert_after = Some(imp);
            }
        }
        if let Some(prev) = insert_after {
            return (
                ByteSpan::new(prev.span.end, prev.span.end),
                format!("\nimport \"{}\";", path),
            );
        }
        // Insert before the first import.
        let first = pf.ast.imports.first().unwrap();
        return (
            ByteSpan::new(first.span.start, first.span.start),
            format!("import \"{}\";\n", path),
        );
    }

    // No imports — insert after the package / syntax line.
    if let Some(pkg) = &pf.ast.package {
        return (
            ByteSpan::new(pkg.span.end, pkg.span.end),
            format!("\n\nimport \"{}\";", path),
        );
    }
    if let Some(span) = pf.ast.syntax_span {
        return (
            ByteSpan::new(span.end, span.end),
            format!("\n\nimport \"{}\";", path),
        );
    }
    // File is empty / unusual — insert at top.
    (ByteSpan::new(0, 0), format!("import \"{}\";\n", path))
}
