//! Hierarchical document-symbol tree for the outline view.

use crate::ast;
use crate::spans::ByteSpan;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentSymbol {
    pub name: String,
    pub detail: String,
    pub kind: SymbolKind,
    pub range: ByteSpan,
    pub selection_range: ByteSpan,
    pub children: Vec<DocumentSymbol>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SymbolKind {
    Target,
    PatternTarget,
    PhonyTarget,
    Variable,
    Constant,
    Include,
    Conditional,
    Directive,
}

pub fn document_symbols(file: &ast::File) -> Vec<DocumentSymbol> {
    items_to_symbols(&file.items)
}

fn items_to_symbols(items: &[ast::Item]) -> Vec<DocumentSymbol> {
    let mut out = Vec::new();
    for item in items {
        match item {
            ast::Item::Rule(r) => out.push(rule_symbol(r)),
            ast::Item::Assignment(a) => out.push(assignment_symbol(a)),
            ast::Item::Define(d) => out.push(define_symbol(d)),
            ast::Item::Include(i) => out.push(include_symbol(i)),
            ast::Item::Conditional(c) => out.push(conditional_symbol(c)),
            ast::Item::Directive(d) => out.push(directive_symbol(d)),
        }
    }
    out
}

fn rule_symbol(r: &ast::Rule) -> DocumentSymbol {
    let primary = r
        .targets
        .first()
        .map(|t| t.name.clone())
        .unwrap_or_default();
    let kind = if r.is_pattern {
        SymbolKind::PatternTarget
    } else if r.is_phony {
        SymbolKind::PhonyTarget
    } else {
        SymbolKind::Target
    };
    let mut detail_parts: Vec<String> = Vec::new();
    if r.targets.len() > 1 {
        let extras: Vec<&str> = r
            .targets
            .iter()
            .skip(1)
            .map(|t| t.name.as_str())
            .collect();
        detail_parts.push(extras.join(" "));
    }
    if !r.prerequisites.is_empty() {
        let deps: Vec<&str> = r.prerequisites.iter().map(|d| d.name.as_str()).collect();
        detail_parts.push(format!(": {}", deps.join(" ")));
    }
    if r.is_double_colon {
        detail_parts.push("::".to_string());
    }
    let detail = detail_parts.join(" ");

    DocumentSymbol {
        name: primary,
        detail,
        kind,
        range: r.span,
        selection_range: r.name_span,
        children: Vec::new(),
    }
}

fn assignment_symbol(a: &ast::Assignment) -> DocumentSymbol {
    let preview = truncate_preview(&a.value, 60);
    let detail = format!("{} {}", a.op.as_str(), preview);
    DocumentSymbol {
        name: a.name.name.clone(),
        detail,
        kind: SymbolKind::Variable,
        range: a.span,
        selection_range: a.name.span,
        children: Vec::new(),
    }
}

fn define_symbol(d: &ast::Define) -> DocumentSymbol {
    let detail = match d.op {
        Some(op) => format!("define {}", op.as_str()),
        None => "define".to_string(),
    };
    DocumentSymbol {
        name: d.name.name.clone(),
        detail,
        kind: SymbolKind::Constant,
        range: d.span,
        selection_range: d.name_span,
        children: Vec::new(),
    }
}

fn include_symbol(i: &ast::Include) -> DocumentSymbol {
    let name = if i.paths.len() == 1 {
        i.paths[0].clone()
    } else {
        i.paths.join(" ")
    };
    let detail = if i.optional {
        "-include".to_string()
    } else {
        "include".to_string()
    };
    DocumentSymbol {
        name,
        detail,
        kind: SymbolKind::Include,
        range: i.span,
        selection_range: i.span,
        children: Vec::new(),
    }
}

fn conditional_symbol(c: &ast::Conditional) -> DocumentSymbol {
    let mut children = items_to_symbols(&c.then_branch);
    if !c.else_branch.is_empty() {
        children.extend(items_to_symbols(&c.else_branch));
    }
    let name = format!("{} {}", c.kind.as_str(), truncate_preview(&c.condition, 50));
    DocumentSymbol {
        name,
        detail: "conditional".to_string(),
        kind: SymbolKind::Conditional,
        range: c.span,
        selection_range: c.name_span,
        children,
    }
}

fn directive_symbol(d: &ast::Directive) -> DocumentSymbol {
    DocumentSymbol {
        name: d.kind.as_str().to_string(),
        detail: truncate_preview(&d.arguments, 60),
        kind: SymbolKind::Directive,
        range: d.span,
        selection_range: d.name_span,
        children: Vec::new(),
    }
}

fn truncate_preview(s: &str, max: usize) -> String {
    let collapsed: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= max {
        collapsed
    } else {
        let mut out: String = collapsed.chars().take(max).collect();
        out.push('…');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse;
    use crate::vfs::FileUri;

    #[test]
    fn outline_includes_rule_and_assignment() {
        let src = "CFLAGS = -O2\nall: main.o\n\tgcc -o all main.o\n";
        let parsed = parse(FileUri::new("test"), src.to_string());
        let symbols = document_symbols(&parsed.ast);
        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[0].kind, SymbolKind::Variable);
        assert_eq!(symbols[0].name, "CFLAGS");
        assert_eq!(symbols[1].kind, SymbolKind::Target);
        assert_eq!(symbols[1].name, "all");
    }

    #[test]
    fn outline_marks_pattern_target() {
        let src = "%.o: %.c\n\tgcc -c $< -o $@\n";
        let parsed = parse(FileUri::new("test"), src.to_string());
        let symbols = document_symbols(&parsed.ast);
        assert_eq!(symbols[0].kind, SymbolKind::PatternTarget);
    }

    #[test]
    fn outline_collects_conditional_children() {
        let src = "ifeq ($(OS),Linux)\nA = 1\nelse\nB = 2\nendif\n";
        let parsed = parse(FileUri::new("test"), src.to_string());
        let symbols = document_symbols(&parsed.ast);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].kind, SymbolKind::Conditional);
        assert_eq!(symbols[0].children.len(), 2);
    }
}
