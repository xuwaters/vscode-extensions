# 0005 — `css` templates keep using `vscode-css-languageservice`

**Status**: Accepted · **Date**: 2026-08-22

## Context

FAST components style themselves with a `` css` ` `` tagged template. fast-analyzer runs
`vscode-css-languageservice` over those, giving validation (`no-invalid-css`), completion, hover,
folding and colour information — for free, from a library Microsoft maintains for VS Code itself.

The Rust ecosystem has CSS parsers — `lightningcss`, `biome_css_parser` — but a *parser* is not a
*language service*. What we use is the diagnostics, the property/value completion data, and the MDN
documentation attached to it.

## Decision

CSS stays in TypeScript. `vscode-css-languageservice` is a runtime dependency of the plugin.

## Consequences

**Three of the corpus's templates are CSS**, against 26 HTML. This is not where the value is, and
spending a Rust CSS language service to get it would be spending the project's budget on its smallest
feature.

**We inherit VS Code's CSS behaviour exactly**, including its property data, its browser-compat
notes, and its bug fixes. A hand-rolled Rust equivalent would diverge from what the user sees in a
`.css` file, and being *differently* right is worse than being the same.

**The plugin bundle carries a JavaScript dependency.** Given [0001](0001-tsserver-plugin-not-lsp.md)
puts a TypeScript layer there regardless, this is not a new kind of cost.

**The boundary is clean**: `css` documents never enter the Rust engine at all. The plugin extracts
them from the AST, hands them to the CSS service, and merges the resulting diagnostics with the
engine's. The virtual-document substitution is shared, because a `` css` ` `` template can contain
`${…}` too.

**Colour information has two sources** — the CSS service inside `` css` ` ``, and our own tree inside
`` html` ` `` style attributes. They meet in the extension host's colour provider
([features.md §11](../design/features.md#11-colour-decorators)).

## Alternatives

**`lightningcss` for validation only.** Would give `no-invalid-css` in Rust and nothing else, at the
cost of a large dependency and error messages that do not match VS Code's. The rule is one of 26 and
not the interesting one.

**Drop CSS support.** It is real parity surface and it is nearly free. No.

## Revisit if

- `vscode-css-languageservice` becomes a packaging problem — it is the largest JavaScript dependency
  in the plugin bundle.
- FAST introduces CSS syntax of its own that the service rejects. `cssPartial` and the existing
  composition helpers are ordinary CSS; if that changes, we would need a pre-pass, not a replacement.
