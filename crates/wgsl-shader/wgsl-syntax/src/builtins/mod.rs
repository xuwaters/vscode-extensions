//! What WGSL already defines.
//!
//! Read by the lexer (to tell a type name from an identifier), by completion,
//! by hover, and by signature help. Keeping it in the syntax crate rather than
//! the server keeps the lexer's word classification and the completion list
//! from drifting apart — they are the same data.
//!
//! **WGSL only.** There was a hand-curated GLSL table beside this one — 407
//! lines, one signature per function, no overloads, no version gating. RFC 012
//! replaced it with `glsl-spec`, which is generated from the reference pages
//! and knows which of `texture`'s thirty-odd signatures a given `#version`
//! has. [`Language`] keeps its `Glsl` variant because it is what tags a
//! document; every accessor here answers for WGSL and returns nothing for it.

pub mod wgsl;

use crate::Language;
use crate::lexer::TokenKind;

/// A name the language defines, with enough detail to render a hover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Builtin {
    pub name: &'static str,
    /// How it is called or declared, for the hover's code block.
    pub signature: &'static str,
    /// One line of prose. Empty when the name speaks for itself.
    pub doc: &'static str,
}

/// Build a `&'static [Builtin]` without repeating the field names.
macro_rules! builtins {
    ($($name:literal, $sig:literal, $doc:literal;)*) => {
        &[$($crate::builtins::Builtin { name: $name, signature: $sig, doc: $doc },)*]
    };
}
pub(crate) use builtins;

/// The builtin functions of a language.
pub fn functions(language: Language) -> &'static [Builtin] {
    match language {
        Language::Wgsl => wgsl::FUNCTIONS,
        Language::Glsl => &[],
    }
}

/// The builtin type names of a language.
pub fn type_names(language: Language) -> Vec<&'static str> {
    match language {
        Language::Wgsl => wgsl::TYPES.to_vec(),
        Language::Glsl => Vec::new(),
    }
}

/// The reserved words of a language.
pub fn keywords(language: Language) -> &'static [&'static str] {
    match language {
        Language::Wgsl => wgsl::KEYWORDS,
        Language::Glsl => &[],
    }
}

/// Predeclared variables. WGSL has none; GLSL's `gl_*` family lives in
/// `glsl_spec::VARIABLES`.
pub fn variables(_language: Language) -> &'static [Builtin] {
    &[]
}

/// Look a builtin function up by name.
pub fn function(language: Language, name: &str) -> Option<&'static Builtin> {
    functions(language).iter().find(|b| b.name == name)
}

/// Look a predeclared variable up by name.
pub fn variable(language: Language, name: &str) -> Option<&'static Builtin> {
    variables(language).iter().find(|b| b.name == name)
}

/// Whether `name` is a builtin type in this language.
pub fn is_type(language: Language, name: &str) -> bool {
    match language {
        Language::Wgsl => wgsl::TYPES.contains(&name),
        Language::Glsl => false,
    }
}

/// Whether `name` is reserved.
pub fn is_keyword(language: Language, name: &str) -> bool {
    keywords(language).contains(&name)
}

/// Whether the language defines `name` at all — a rename must refuse these.
pub fn is_reserved(language: Language, name: &str) -> bool {
    is_keyword(language, name)
        || is_type(language, name)
        || function(language, name).is_some()
        || variable(language, name).is_some()
}

/// How the lexer classifies a word.
///
/// Builtin *functions* deliberately stay [`TokenKind::Ident`]: they are
/// shadowable in GLSL, they read as calls rather than keywords, and semantic
/// highlighting paints them from the symbol index instead.
pub fn classify_word(text: &str, language: Language) -> TokenKind {
    if is_keyword(language, text) {
        TokenKind::Keyword
    } else if is_type(language, text) {
        TokenKind::Type
    } else {
        TokenKind::Ident
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_has_a_signature() {
        for language in [Language::Wgsl, Language::Glsl] {
            for builtin in functions(language) {
                assert!(!builtin.signature.is_empty(), "{}", builtin.name);
                assert!(
                    builtin.signature.contains(builtin.name),
                    "{} signature does not name it: {}",
                    builtin.name,
                    builtin.signature
                );
            }
        }
    }

    #[test]
    fn builtin_names_are_unique() {
        for language in [Language::Wgsl, Language::Glsl] {
            let mut names: Vec<&str> = functions(language).iter().map(|b| b.name).collect();
            let before = names.len();
            names.sort_unstable();
            names.dedup();
            assert_eq!(names.len(), before, "duplicate builtin in {language:?}");
        }
    }

}
