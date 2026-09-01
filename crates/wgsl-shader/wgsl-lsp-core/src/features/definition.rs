//! `textDocument/definition`.
//!
//! Three ways to land on an answer, tried in order:
//!
//! 1. **A member access.** `camera.view` is not resolvable by name — `view`
//!    could be a member of any struct in the file. The analyzer types the
//!    base, and the field is looked up in *that* struct's declaration.
//! 2. **A name in scope.** The syntax layer's scope rule, which works whether
//!    or not the file currently parses.
//! 3. **Another file.** Shaders are usually single files, but a project that
//!    generates or `#include`s them is not, so the workspace index gets a look
//!    before we give up.
//!
//! For GLSL the first two collapse into one: `glsl-analysis` already resolved
//! every occurrence in the file to a symbol, a field, a builtin or nothing, so
//! the answer is read off that rather than recomputed here. Shadowing and
//! macro definition sites come with it.

use lsp_types::{GotoDefinitionParams, GotoDefinitionResponse, Location};

use crate::Server;
use crate::analysis::types;
use crate::features::member_chain;
use crate::state::Document;

mod glsl;

impl Server {
    pub fn definition(
        &mut self,
        params: GotoDefinitionParams,
    ) -> Option<GotoDefinitionResponse> {
        let position = params.text_document_position_params;
        let (document, offset) = self.locate(&position.text_document.uri, position.position)?;

        if document.glsl().is_some() {
            if let Some(span) = glsl::declaration_span(document, offset) {
                return Some(GotoDefinitionResponse::Scalar(Location {
                    uri: document.uri.clone(),
                    range: document.range(span),
                }));
            }
        } else if let Some(index) = target(document, offset) {
            return Some(GotoDefinitionResponse::Scalar(Location {
                uri: document.uri.clone(),
                range: document.range(document.parsed().symbols[index].name_span),
            }));
        }

        // Nothing in this file. The name may still be declared in one the
        // editor has not opened.
        let reference = document.parsed().reference_at(offset)?;
        let name = document.slice(reference.span);
        let locations: Vec<Location> = self
            .index()
            .entries()
            .iter()
            .filter(|entry| entry.name == name)
            .map(|entry| Location { uri: entry.uri.clone(), range: entry.selection_range })
            .collect();

        (!locations.is_empty()).then_some(GotoDefinitionResponse::Array(locations))
    }
}

/// The symbol the cursor's identifier declares or refers to, within this file.
pub(crate) fn target(document: &Document, offset: u32) -> Option<usize> {
    let parsed = document.parsed();

    // The cursor is on a declaration's own name: that *is* the definition.
    if let Some(index) = parsed.symbol_declared_at(offset) {
        return Some(index);
    }

    let reference = parsed.reference_at(offset)?;
    if reference.is_member {
        return member_target(document, offset, document.slice(reference.span));
    }
    parsed.resolve_at(document.text(), offset)
}

/// The declaration of `field`, reached through the type of the chain before it.
fn member_target(document: &Document, offset: u32, field: &str) -> Option<usize> {
    let parsed = document.parsed();
    let module = document.module()?;
    let (base, fields) = member_chain(parsed, document.text(), offset)?;
    let function = document.naga_function_at(&module, offset);

    let resolution = types::type_of_chain(&module, function, base, &fields)?;
    // The struct has to be a *named* one for the declaration to be findable:
    // an anonymous type has nothing in the source to jump to.
    let handle = resolution.handle()?;
    let struct_name = module.types[handle].name.as_deref()?;

    let owner = parsed.symbols.iter().position(|symbol| {
        symbol.name == struct_name
            && matches!(
                symbol.kind,
                wgsl_syntax::SymbolKind::Struct | wgsl_syntax::SymbolKind::Block
            )
    })?;
    parsed.symbols[owner]
        .children
        .iter()
        .copied()
        .find(|&child| parsed.symbols[child].name == field)
}
