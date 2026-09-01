//! Projecting the GLSL outline onto the shapes the feature layer consumes.
//!
//! The feature layer was written against [`wgsl_syntax::Parsed`] — a flat
//! symbol arena with a scope span apiece, one reference per identifier
//! occurrence, and the delimiter blocks folding and signature help read. The
//! GLSL outline produces the same three things with the same meanings
//! ([`glsl_syntax::outline`]), so this is a field-for-field mapping, not a
//! translation: [`glsl_syntax::SymbolKind`] mirrors [`wgsl_syntax::SymbolKind`]
//! name for name minus `TypeAlias`, which GLSL has no spelling for.
//!
//! Two things are computed here rather than carried across, because the two
//! token models differ:
//!
//! - **Token kinds.** The GLSL lexer deliberately does not separate keywords
//!   from identifiers — which words are reserved depends on the `#version`,
//!   which is not known until it has been read. Classification therefore
//!   happens here, against `glsl-spec`'s tables.
//! - **Blocks.** `(`/`[`/`{` are paired over the raw stream, exactly as the
//!   WGSL parser pairs them, so an unclosed one still yields the blocks before
//!   it. The CST could answer this for a *valid* file; the editor asks about
//!   invalid ones.

use analyzer_core::spans::ByteSpan;
use glsl_syntax::lexer::{Punct, Token as GlslToken, TokenKind as GlslKind};
use glsl_syntax::outline::{Outline, SymbolKind as GlslSymbolKind};
use wgsl_syntax::lexer::{Token, TokenKind};
use wgsl_syntax::{Block, BlockKind, Language, Parsed, Reference, Symbol, SymbolKind};

/// Project a preprocessed, parsed GLSL source onto the feature layer's shapes.
pub fn to_parsed(source: &str, raw: &[GlslToken], outline: &Outline) -> Parsed {
    let tokens = tokens(source, raw);
    let mut all_blocks = blocks(source, &tokens);
    all_blocks.extend(comment_blocks(source, &tokens));
    all_blocks.sort_by_key(|b| (b.span.start, std::cmp::Reverse(b.span.end)));

    Parsed {
        language: Language::Glsl,
        tokens,
        symbols: outline.symbols.iter().map(symbol).collect(),
        roots: outline.roots.clone(),
        references: outline.references.iter().map(reference).collect(),
        blocks: all_blocks,
        // Diagnostics come from the preprocessor, the parser and semantic
        // analysis, each with its own `GLSL####` code; the projection invents
        // none of its own.
        diagnostics: Vec::new(),
    }
}

fn symbol(symbol: &glsl_syntax::outline::Symbol) -> Symbol {
    Symbol {
        name: symbol.name.clone(),
        kind: symbol_kind(symbol.kind),
        name_span: symbol.name_span,
        full_span: symbol.full_span,
        scope: symbol.scope,
        detail: symbol.detail.clone(),
        parent: symbol.parent,
        children: symbol.children.clone(),
    }
}

/// The kinds line up name for name. GLSL has no `TypeAlias`, so nothing maps
/// to it — which is why this is exhaustive rather than a fallback arm.
pub fn symbol_kind(kind: GlslSymbolKind) -> SymbolKind {
    match kind {
        GlslSymbolKind::Function => SymbolKind::Function,
        GlslSymbolKind::EntryPoint => SymbolKind::EntryPoint,
        GlslSymbolKind::Struct => SymbolKind::Struct,
        GlslSymbolKind::Field => SymbolKind::Field,
        GlslSymbolKind::Variable => SymbolKind::Variable,
        GlslSymbolKind::Constant => SymbolKind::Constant,
        GlslSymbolKind::Parameter => SymbolKind::Parameter,
        GlslSymbolKind::Local => SymbolKind::Local,
        GlslSymbolKind::Macro => SymbolKind::Macro,
        GlslSymbolKind::Block => SymbolKind::Block,
    }
}

fn reference(reference: &glsl_syntax::outline::Reference) -> Reference {
    Reference {
        span: reference.span,
        is_declaration: reference.is_declaration,
        is_member: reference.is_member,
    }
}

/// The raw stream, minus whitespace, with each word classified.
///
/// A directive's `#` and its keyword become one [`TokenKind::Preprocessor`]
/// token, matching what the WGSL-side lexer produced for GLSL: completion
/// keys its "after a `#`" context off exactly that.
fn tokens(source: &str, raw: &[GlslToken]) -> Vec<Token> {
    let mut tokens: Vec<Token> = Vec::with_capacity(raw.len() / 2 + 1);
    let mut index = 0;
    while index < raw.len() {
        let token = raw[index];
        index += 1;
        let kind = match token.kind {
            GlslKind::Space | GlslKind::Newline | GlslKind::LineContinuation => continue,
            GlslKind::Comment => TokenKind::Comment,
            GlslKind::Int | GlslKind::Float => TokenKind::Number,
            GlslKind::Str => TokenKind::Str,
            GlslKind::Unknown => TokenKind::Unknown,
            GlslKind::Punct(Punct::Hash) if token.at_line_start => {
                // `#` then the directive word, if one follows on the same
                // line, as one token: `#version`, `#  define`.
                let mut end = token.span.end;
                let mut scan = index;
                while let Some(next) = raw.get(scan) {
                    match next.kind {
                        GlslKind::Space => scan += 1,
                        GlslKind::Ident => {
                            end = next.span.end;
                            index = scan + 1;
                            break;
                        }
                        _ => break,
                    }
                }
                tokens.push(Token {
                    kind: TokenKind::Preprocessor,
                    span: ByteSpan::new(token.span.start, end),
                    line: token.line,
                    at_line_start: token.at_line_start,
                });
                continue;
            }
            GlslKind::Punct(_) => TokenKind::Punct,
            GlslKind::Ident => classify(&token.text(source)),
        };
        tokens.push(Token {
            kind,
            span: token.span,
            line: token.line,
            at_line_start: token.at_line_start,
        });
    }
    tokens
}

/// What a word is.
///
/// Types before keywords: `glsl_spec::is_keyword` counts a basic type as one,
/// and the editor paints a type differently from a `for`. Builtin *functions*
/// stay identifiers — they are shadowable, they read as calls, and semantic
/// highlighting paints them from the resolved reference instead.
///
/// Deliberately version-blind. A word reserved in 4.60 and free in 1.10 is
/// still worth colouring as reserved, and the availability *diagnostic* is
/// what tells the user which version they are in.
fn classify(text: &str) -> TokenKind {
    if glsl_spec::basic_type(text).is_some() {
        TokenKind::Type
    } else if glsl_spec::keyword(text).is_some() {
        TokenKind::Keyword
    } else {
        TokenKind::Ident
    }
}

/// Pair `(`/`)`, `[`/`]` and `{`/`}` over the whole file.
///
/// `<`/`>` are deliberately absent: they are comparisons as often as they are
/// anything else, and guessing wrong corrupts the block structure folding and
/// signature help depend on.
fn blocks(source: &str, tokens: &[Token]) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut stack: Vec<(ByteSpan, u8)> = Vec::new();
    for token in tokens {
        if token.kind != TokenKind::Punct || token.span.len() != 1 {
            continue;
        }
        let byte = source.as_bytes()[token.span.start as usize];
        match byte {
            b'(' | b'[' | b'{' => stack.push((token.span, byte)),
            b')' | b']' | b'}' => {
                let want = match byte {
                    b')' => b'(',
                    b']' => b'[',
                    _ => b'{',
                };
                // Pop through anything the source left open, so a stray `{`
                // does not swallow every block after it.
                if let Some(at) = stack.iter().rposition(|&(_, open)| open == want) {
                    let (open_span, _) = stack[at];
                    stack.truncate(at);
                    blocks.push(Block {
                        kind: block_kind(want),
                        span: ByteSpan::new(open_span.start, token.span.end),
                    });
                }
            }
            _ => {}
        }
    }
    blocks
}

fn block_kind(open: u8) -> BlockKind {
    match open {
        b'(' => BlockKind::Paren,
        b'[' => BlockKind::Bracket,
        _ => BlockKind::Brace,
    }
}

/// Every multi-line `/* … */`, and every run of two or more adjacent `//`
/// lines — the regions a reader actually wants to collapse.
fn comment_blocks(source: &str, tokens: &[Token]) -> Vec<Block> {
    let comments: Vec<&Token> =
        tokens.iter().filter(|t| t.kind == TokenKind::Comment).collect();
    let mut blocks = Vec::new();
    let mut i = 0;
    while i < comments.len() {
        let start = comments[i];
        let text = &source[start.span.start as usize..start.span.end as usize];
        if !text.starts_with("//") {
            if text.contains('\n') {
                blocks.push(Block { kind: BlockKind::Comment, span: start.span });
            }
            i += 1;
            continue;
        }
        let mut end = i;
        while end + 1 < comments.len()
            && comments[end + 1].line == comments[end].line + 1
            && source[comments[end + 1].span.start as usize..].starts_with("//")
        {
            end += 1;
        }
        if end > i {
            blocks.push(Block {
                kind: BlockKind::Comment,
                span: ByteSpan::new(start.span.start, comments[end].span.end),
            });
        }
        i = end + 1;
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(source: &str) -> Parsed {
        let raw = glsl_syntax::tokenize(source);
        let pp = glsl_syntax::preprocess(
            source,
            &raw,
            &glsl_syntax::PreprocessOptions::default(),
        );
        let tree = glsl_syntax::parse(&pp);
        let outline = glsl_syntax::outline::outline(&tree, &pp, source);
        to_parsed(source, &raw, &outline)
    }

    #[test]
    fn a_directive_is_one_token_and_the_rest_of_the_line_is_not() {
        let source = "#version 450\n#  define PI 3.14\n";
        let parsed = project(source);
        let kinds: Vec<(TokenKind, &str)> = parsed
            .tokens
            .iter()
            .map(|t| (t.kind, parsed.text(source, t.span)))
            .collect();
        assert_eq!(kinds[0], (TokenKind::Preprocessor, "#version"));
        assert_eq!(kinds[1], (TokenKind::Number, "450"));
        assert_eq!(kinds[2], (TokenKind::Preprocessor, "#  define"));
        assert_eq!(kinds[3], (TokenKind::Ident, "PI"));
        assert_eq!(kinds[4], (TokenKind::Number, "3.14"));
    }

    /// A bare `#`, which is what completion is triggered on.
    #[test]
    fn a_hash_with_nothing_after_it_is_still_a_directive() {
        let source = "#version 450\n#\n";
        let parsed = project(source);
        let last = parsed.tokens.last().unwrap();
        assert_eq!(last.kind, TokenKind::Preprocessor);
        assert_eq!(parsed.text(source, last.span), "#");
    }

    #[test]
    fn words_are_classified_against_the_spec_tables() {
        let source = "uniform sampler2D albedo;\n";
        let parsed = project(source);
        let kinds: Vec<TokenKind> = parsed.tokens.iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            [TokenKind::Keyword, TokenKind::Type, TokenKind::Ident, TokenKind::Punct]
        );
    }

    #[test]
    fn delimiters_pair_across_the_whole_file_and_survive_an_unclosed_one() {
        let source = "void main() {\n  if (x) { y(); }\n";
        let parsed = project(source);
        let parens = parsed.blocks.iter().filter(|b| b.kind == BlockKind::Paren).count();
        let braces = parsed.blocks.iter().filter(|b| b.kind == BlockKind::Brace).count();
        assert_eq!(parens, 3, "{:?}", parsed.blocks);
        // The inner `{ y(); }` closes; the function body never does.
        assert_eq!(braces, 1);
    }

    #[test]
    fn a_run_of_line_comments_folds_and_a_lone_one_does_not() {
        let source = "// one\n// two\nvoid main() {}\n// alone\n";
        let parsed = project(source);
        let comments: Vec<ByteSpan> = parsed
            .blocks
            .iter()
            .filter(|b| b.kind == BlockKind::Comment)
            .map(|b| b.span)
            .collect();
        assert_eq!(comments.len(), 1);
        assert_eq!(parsed.text(source, comments[0]), "// one\n// two");
    }

    /// The whole point of the projection: the feature layer's own queries
    /// answer on a GLSL document without knowing anything changed.
    #[test]
    fn symbols_scopes_and_references_answer_the_feature_layers_queries() {
        let source = "#version 450\n\
            uniform float scale;\n\
            float doubled(float x) { float y = x * scale; return y; }\n";
        let parsed = project(source);

        let names: Vec<&str> = parsed.symbols.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["scale", "doubled", "x", "y"]);
        assert_eq!(parsed.symbols[1].kind, SymbolKind::Function);
        assert_eq!(parsed.symbols[2].kind, SymbolKind::Parameter);
        assert_eq!(parsed.symbols[3].kind, SymbolKind::Local);

        // Resolution through the scope spans, which is what definition and
        // rename ask for.
        let at = source.find("return y").unwrap() as u32 + 7;
        assert_eq!(parsed.resolve_at(source, at), Some(3));
        assert_eq!(parsed.occurrences(source, "scale").count(), 2);
        assert_eq!(parsed.enclosing_function(at), Some(1));
    }

    /// A macro invocation is one reference to the macro, at the name the user
    /// wrote — never at the body the preprocessor substituted.
    #[test]
    fn a_macro_invocation_references_the_macro_by_name() {
        let source = "#define HALF 0.5\nfloat x = HALF;\n";
        let parsed = project(source);
        let uses: Vec<&str> = parsed
            .occurrences(source, "HALF")
            .map(|r| parsed.text(source, r.span))
            .collect();
        assert_eq!(uses, ["HALF", "HALF"]);
        assert_eq!(parsed.symbols[0].kind, SymbolKind::Macro);
    }
}
