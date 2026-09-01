//! Syntax for WGSL and GLSL, built to survive incomplete input.
//!
//! The server pairs this crate with naga. naga is authoritative — it type-checks,
//! and it produces the diagnostics — but it answers only for sources it can
//! parse completely, which excludes every half-typed line and, for GLSL, every
//! dialect outside the vertex/fragment/compute stages at `#version 440`, `450`
//! and `460 core`. This crate answers for everything, less precisely:
//!
//! - [`lexer::tokenize`] never fails, and classifies each word per language.
//! - [`parse`] finds declarations, the scope each name is visible in, and every
//!   identifier occurrence, recovering at the next `;` or `}` from anything it
//!   does not recognise.
//! - [`builtins`] holds what the languages predeclare, with a signature and a
//!   line of documentation apiece.
//!
//! ```
//! use wgsl_syntax::{Language, parse};
//!
//! let parsed = parse("fn main() { let x = 1; ", Language::Wgsl);
//! // The source is missing its closing brace, and the outline is still right.
//! assert_eq!(parsed.symbols[0].name, "main");
//! assert!(parsed.diagnostics.iter().any(|d| d.message.contains("unclosed")));
//! ```

pub mod builtins;
pub mod lexer;
mod parser;
pub mod tree;

#[cfg(test)]
mod tests;

pub use tree::{
    Block, BlockKind, Parsed, Reference, Symbol, SymbolKind, SyntaxDiagnostic,
};

/// Which of the two shading languages a source is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    Wgsl,
    Glsl,
}

impl Language {
    /// The language for a VS Code language id.
    pub fn from_id(id: &str) -> Option<Language> {
        match id {
            "wgsl" => Some(Language::Wgsl),
            "glsl" => Some(Language::Glsl),
            _ => None,
        }
    }

    /// The VS Code language id.
    pub fn id(self) -> &'static str {
        match self {
            Language::Wgsl => "wgsl",
            Language::Glsl => "glsl",
        }
    }

    /// The name to show a human.
    pub fn label(self) -> &'static str {
        match self {
            Language::Wgsl => "WGSL",
            Language::Glsl => "GLSL",
        }
    }

    /// The language a file extension implies, without the dot.
    ///
    /// The GLSL list matches the extensions the extension's `package.json`
    /// registers, so anything the editor calls GLSL is treated as GLSL here.
    pub fn from_extension(extension: &str) -> Option<Language> {
        match extension {
            "wgsl" => Some(Language::Wgsl),
            "glsl" | "vert" | "frag" | "comp" | "geom" | "tesc" | "tese" | "vsh" | "fsh"
            | "gsh" | "vshader" | "fshader" | "gshader" | "glslv" | "glslf" | "vertexshader"
            | "fragmentshader" | "vs" | "fs" | "cs" | "csh" | "mesh" | "task" => {
                Some(Language::Glsl)
            }
            _ => None,
        }
    }
}

/// Lex and parse a source.
pub fn parse(source: &str, language: Language) -> Parsed {
    let tokens = lexer::tokenize(source, language);
    parser::parse(source, language, tokens)
}
