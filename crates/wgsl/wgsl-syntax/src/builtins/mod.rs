//! What the languages already define.
//!
//! One table per language, read by the lexer (to tell a type name from an
//! identifier), by completion, by hover, and by signature help. Keeping it in
//! the syntax crate rather than the server keeps the lexer's word
//! classification and the completion list from drifting apart — they are the
//! same data.
//!
//! Ported from the extension's former `src/shaderData.ts`, with a signature and
//! a one-line doc added to every function so hover and signature help have
//! something to show.

pub mod glsl;
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
        Language::Glsl => glsl::FUNCTIONS,
    }
}

/// The builtin type names of a language.
///
/// GLSL's opaque types (`sampler2D`, `uimage2DArray`, `samplerCubeShadow`, …)
/// are generated rather than listed — there are several hundred, and they are
/// systematic. See [`glsl::opaque_type_names`].
pub fn type_names(language: Language) -> Vec<&'static str> {
    match language {
        Language::Wgsl => wgsl::TYPES.to_vec(),
        Language::Glsl => {
            let mut names = glsl::TYPES.to_vec();
            names.extend(glsl::opaque_type_names());
            names
        }
    }
}

/// The reserved words of a language.
pub fn keywords(language: Language) -> &'static [&'static str] {
    match language {
        Language::Wgsl => wgsl::KEYWORDS,
        Language::Glsl => glsl::KEYWORDS,
    }
}

/// Predeclared variables: WGSL has none, GLSL has the `gl_*` family.
pub fn variables(language: Language) -> &'static [Builtin] {
    match language {
        Language::Wgsl => &[],
        Language::Glsl => glsl::VARIABLES,
    }
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
        Language::Glsl => glsl::TYPES.contains(&name) || glsl::is_opaque_type(name),
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
    fn words_are_classified_per_language() {
        // `fn` is a WGSL keyword and an ordinary word in GLSL.
        assert_eq!(classify_word("fn", Language::Wgsl), TokenKind::Keyword);
        assert_eq!(classify_word("fn", Language::Glsl), TokenKind::Ident);
        // `uniform` the other way round.
        assert_eq!(classify_word("uniform", Language::Glsl), TokenKind::Keyword);
        assert_eq!(classify_word("uniform", Language::Wgsl), TokenKind::Ident);

        assert_eq!(classify_word("vec4f", Language::Wgsl), TokenKind::Type);
        assert_eq!(classify_word("sampler2D", Language::Glsl), TokenKind::Type);
        assert_eq!(classify_word("myThing", Language::Wgsl), TokenKind::Ident);
    }

    /// Builtin calls must not be painted as keywords, or every `dot(a, b)`
    /// turns the same colour as `return`.
    #[test]
    fn builtin_functions_lex_as_identifiers() {
        assert_eq!(classify_word("dot", Language::Wgsl), TokenKind::Ident);
        assert_eq!(classify_word("texture", Language::Glsl), TokenKind::Ident);
        assert!(function(Language::Wgsl, "dot").is_some());
        assert!(function(Language::Glsl, "texture").is_some());
    }

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

    #[test]
    fn reserved_covers_keywords_types_and_builtins() {
        assert!(is_reserved(Language::Wgsl, "fn"));
        assert!(is_reserved(Language::Wgsl, "vec3f"));
        assert!(is_reserved(Language::Wgsl, "textureSample"));
        assert!(is_reserved(Language::Glsl, "gl_Position"));
        assert!(!is_reserved(Language::Wgsl, "myUniform"));
    }
}
