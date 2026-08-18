# 0013 — Colour language-tagged raw blocks with the embedded language's own grammar

**Status**: Accepted
**Date**: 2026-08-18
**Amends**: [0007](0007-textmate-grammar.md)

## Context

A typst document about code is mostly code:

````typst
```rust
fn main() { println!("hi"); }
```
````

Typst itself highlights that block in the *rendered* output — the compiler carries syntect and a syntax
set. In the *editor* it was a single flat colour, because both of our colouring mechanisms treat a raw
block as one thing:

- The TextMate grammar (0007) scopes the whole fence `markup.raw.block.typst` and stops.
- `typst_syntax::highlight()` returns `Tag::Raw` for the raw element, so semantic tokens paint every line
  of the body `raw`.

The second is the one that matters, and it is the trap: **semantic tokens outrank TextMate scopes in
VSCode.** Teaching the grammar about `` ```rust `` while the server keeps emitting `raw` over the body
would have produced exactly the same flat colour, and looked like a grammar bug.

## Decision

**A raw block that names a language belongs to that language's grammar, end to end.**

Three parts, all of which have to hold together:

1. **`syntaxes/typst-embedded.tmLanguage.json`** (scope `source.typst.embedded`) carries one begin/end rule
   per language: `` (`{3,})((?i:rust|rs))(?=[\s`]|$) `` … `\1`, `contentName`
   `meta.embedded.block.rust`, contents included from `source.rust`. `source.typst` includes the whole
   grammar *before* its own raw-block rule, so an unknown tag still falls through to plain `markup.raw`.

2. **`contributes.grammars[typst].embeddedLanguages`** maps each `meta.embedded.block.<id>` onto the VSCode
   language id. That is what makes the *editor* — not just the colours — switch: comment toggling,
   bracket matching, and the word pattern inside the block are the embedded language's.

3. **The server emits no semantic token over the body of a tagged block** (`semantic_tokens.rs`). The
   fence and the tag stay `raw`, so the edges match an untagged block; the body is deliberately left
   untagged so nothing paints over the grammar. An *untagged* block has no grammar to defer to and is
   coloured by the server exactly as before.

**The language set is a table, not sixty hand-written rules.** `scripts/embedded/languages.json` holds one
row per language — id, scope, tags, and who provides the grammar — and `scripts/embedded/generate.mjs`
expands it into the grammar file and into `package.json`. Adding a language is one row and
`pnpm run build:grammar`. `--check` re-runs the expansion without writing and fails if either artifact is
stale, which is what the test suite invokes, so a forgotten regeneration cannot ship.

Rows whose grammar VSCode does not bundle (`toml`, `kotlin`, `zig`, our own `wgsl` and `proto3`, …) are
marked `"provider": "extension"` and included anyway: an unresolved include costs nothing — the block just
stays raw-coloured — and lights up the moment the user installs that extension.

## Consequences

**Buys.** Code in a typst document is coloured the way it is coloured everywhere else in the editor, by
the grammar the user already has, for 73 languages and 149 tags. `embeddedLanguages` means `Cmd+/` inside
a `` ```python `` block writes `#`, not `//`. The pattern generalises: a new language is a table row.

**Costs.** This is the grammar-generation build step 0007 declined to take on, in miniature — a script and
a table, run by hand, guarded by a check. It is small because the generated rules are uniform; if raw
blocks were the only thing that needed generating, they would not have justified it, and the honest
reading is that 0007's "no build step" now has an asterisk.

The `(?=[\s`]|$)` after the tag is load-bearing and not obvious: a word boundary would let the `c` rule
match `` ```c++ `` and leave `++` as code, because `+` is not a word character.

**Testing.** `src/grammar.test.ts` runs the real Oniguruma over the real grammar files — the two
constructs that make this work, `(?i:…)` and the closing back-reference, have no JavaScript equivalent, so
a `RegExp` approximation would be testing something else. Where the machine has a VSCode install, the
suite reads its manifests and checks every row's scope, language id, and `provider` against what that
install actually ships. That is the only thing that catches a plausible-but-wrong scope name —
`text.restructuredtext` for what is really `source.rst` — and it caught three.

## Revisit if

- Upstream typst exposes the syntect highlighting it already does for rendering over LSP (a
  `typst/rawHighlight` notification, or `Tag::Raw` gaining the tag). Then the grammar table is redundant:
  colouring would come from the same syntax set that renders the PDF, and the editor would match the
  output exactly rather than approximately.
- The table grows past the point where a flat JSON file is the right shape — if rows start needing
  per-language options, it wants a schema and a proper build step rather than one more optional field.
- VSCode changes how `embeddedLanguages` resolves, or starts warning about includes of scopes no installed
  extension provides. The `"provider": "extension"` rows are the ones that would go noisy.
