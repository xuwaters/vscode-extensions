//! Painting GLSL by what the analyzer resolved (P5-07).
//!
//! The old classifier guessed: it looked a name up in the symbol index, then in
//! a flat builtin table, and painted whatever it hit first. Three things go
//! wrong with that and all three are fixed by asking the analysis instead:
//!
//! - **A local that shadows a builtin** painted as the builtin.
//! - **A member** painted as a property only when a `.` happened to precede
//!   it in the token stream, so `s.colour.rg` lost the swizzle.
//! - **A macro invocation** painted as a plain variable, because the macro is
//!   gone by the time the symbol index is built.

use analyzer_core::spans::ByteSpan;
use glsl_analysis::{SymbolKind, Target};
use wgsl_syntax::Reference;

use super::{modifier, ty};
use crate::state::Document;

/// What to paint the identifier at `span` as.
pub fn identifier(
    document: &Document,
    span: ByteSpan,
    reference: Option<&Reference>,
) -> (u32, u32) {
    let Some(glsl) = document.glsl() else {
        return (ty::VARIABLE, 0);
    };
    let name = document.slice(span);
    let declaring = reference.is_some_and(|r| r.is_declaration);
    let declaration = if declaring { modifier::DECLARATION } else { 0 };

    // A macro first: it is not in the resolved references at all, because the
    // preprocessor consumed it long before analysis ran.
    if glsl.macro_at(name).is_some() {
        return (ty::MACRO, declaration);
    }

    match glsl.analysis.reference_at(span.start).map(|r| &r.target) {
        Some(Target::Symbol(id)) => match glsl.analysis.symbols.get(*id) {
            Some(symbol) => {
                let (token, modifiers) = from_symbol_kind(symbol.kind);
                let read_only = if symbol.qualifiers.is_const || symbol.qualifiers.is_uniform {
                    modifier::READONLY
                } else {
                    0
                };
                (token, modifiers | read_only | declaration)
            }
            None => (ty::VARIABLE, declaration),
        },
        Some(Target::BuiltinFunction(_)) | Some(Target::LegacyFunction(_)) => {
            (ty::FUNCTION, modifier::DEFAULT_LIBRARY)
        }
        Some(Target::BuiltinVariable(_)) | Some(Target::LegacyVariable(_)) => {
            (ty::VARIABLE, modifier::DEFAULT_LIBRARY | modifier::READONLY)
        }
        Some(Target::Type(ty)) => {
            let library = if matches!(ty, glsl_analysis::Type::Struct(_)) {
                0
            } else {
                modifier::DEFAULT_LIBRARY
            };
            let token = if library == 0 { ty::STRUCT } else { ty::TYPE };
            (token, library | declaration)
        }
        Some(Target::Field { .. }) | Some(Target::Swizzle) => (ty::PROPERTY, declaration),
        _ => {
            // No resolved reference. Either this is a declaration's own name,
            // which analysis records as a symbol rather than a use, or the
            // token is inside a branch a conditional switched off and never
            // reached analysis at all. The outline covers both.
            if let Some(index) = document.parsed().symbol_declared_at(span.start) {
                let (token, modifiers) =
                    super::from_symbol(document.parsed().symbols[index].kind);
                return (token, modifiers | declaration);
            }
            // A name nothing placed. Painting it as a plain variable is the
            // honest answer; the diagnostic layer decides whether it is also
            // an error.
            (ty::VARIABLE, declaration)
        }
    }
}

fn from_symbol_kind(kind: SymbolKind) -> (u32, u32) {
    match kind {
        SymbolKind::Function => (ty::FUNCTION, 0),
        SymbolKind::Struct | SymbolKind::Block => (ty::STRUCT, 0),
        SymbolKind::Global => (ty::VARIABLE, 0),
        SymbolKind::Parameter => (ty::PARAMETER, 0),
        SymbolKind::Local => (ty::VARIABLE, 0),
        SymbolKind::Field => (ty::PROPERTY, 0),
    }
}
