//! Where a GLSL name is declared (P5-06).
//!
//! One lookup, not three. `glsl-analysis` resolved every identifier occurrence
//! in the file while it was typing the expressions around it, so the answer is
//! already recorded — shadowing included, because resolution happened with the
//! scope stack in the state that use site saw.
//!
//! The two things a name-based scan could not do, and this does:
//!
//! - **A member lands on the right struct.** `camera.projection` resolves to a
//!   field of `Camera` because the base was typed, not because some struct in
//!   the file happens to have a `projection`.
//! - **A macro lands on its `#define`.** The directive never reaches the
//!   expanded token stream, so only the macro table knows where it was written.

use analyzer_core::spans::ByteSpan;
use glsl_analysis::Target;

use crate::state::Document;

/// The span of the declaring name for whatever is under the cursor.
pub fn declaration_span(document: &Document, offset: u32) -> Option<ByteSpan> {
    let glsl = document.glsl()?;

    // A macro is looked up by name: it is not in the expanded stream at all,
    // and the occurrence under the cursor may be its own `#define`.
    let name = document.parsed().reference_at(offset).map(|r| document.slice(r.span));
    if let Some(def) = name.and_then(|name| glsl.macro_at(name)) {
        return Some(def.name_span);
    }

    match glsl.analysis.reference_at(offset)?.target {
        Target::Symbol(id) => glsl.analysis.symbols.get(id).map(|symbol| symbol.name_span),
        Target::Field { owner, index } => glsl
            .analysis
            .structs
            .get(owner)
            .and_then(|def| def.fields.get(index))
            .map(|field| field.name_span),
        // A type the file declares has a declaration to jump to; a builtin one
        // does not, and neither does a builtin function, a swizzle or a name
        // nothing placed. The struct's *name* is on the symbol that introduced
        // it, which is what an editor wants to select.
        Target::Type(glsl_analysis::Type::Struct(id)) => glsl
            .analysis
            .symbols
            .iter()
            .find(|(_, symbol)| symbol.struct_id == Some(id))
            .map(|(_, symbol)| symbol.name_span),
        _ => None,
    }
}
