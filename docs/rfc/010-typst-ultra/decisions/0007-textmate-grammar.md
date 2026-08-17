# 0007 — Minimal TextMate grammar; semantic tokens do the real work

**Status**: Accepted
**Date**: 2026-08-17
**Resolves**: OQ 1

## Context

VSCode needs a TextMate grammar for a language to have any coloring before a language server responds.
Typst's grammar is genuinely hard to express in TextMate: markup and code modes interleave arbitrarily
(`#` enters code from markup, `[…]` re-enters markup from code, `$…$` enters math), which is exactly the
kind of recursive mode-switching regex-based grammars handle badly.

Tinymist solved this by writing a **TypeScript program that generates** the grammar
([`temp/tinymist/syntaxes/textmate/`](../../../../temp/tinymist/syntaxes/textmate), Apache-2.0) — with its
own build step, snapshot test suite, and test corpora run against typst's own repository. It is good work
and it is a project in itself.

We have something tinymist's grammar generator does not: `typst_syntax::highlight()`, which returns one of
22 `Tag` values per node **from the actual compiler parser**. Coloring derived from it is correct by
construction, not by regex approximation. The spike confirmed it works in WASM (74 tagged nodes on a small
document).

## Decision

Write a **deliberately minimal** TextMate grammar (~200 lines, ours) covering only what must be right
before the server responds: comments, strings, headings, emphasis markers, math delimiters, code-mode
keywords, and numbers. Accept that it will be approximate at mode boundaries.

**Semantic tokens are the primary coloring mechanism**, not a garnish. The server implements both
`semanticTokens/full` and `full/delta`, mapping all 22 tags — using standard LSP token types where they
fit and custom types with `contributes.semanticTokenScopes` TextMate fallbacks where they do not
(`strong`, `emph`, `heading`, `raw`, `label`, `ref`, `link`, `listMarker`, `mathDelimiter`, `escape`,
`interpolated`, `error`). The mapping table is in
[design/lsp-features.md §4.1](../design/lsp-features.md#41-semantic-tokens).

`typstUltra.semanticTokens: "disable"` therefore means "fall back to approximate coloring", and the setting
description says so.

## Consequences

**Buys.** No grammar-generation build step, no snapshot corpus to maintain, and coloring that is *more*
accurate than any TextMate grammar can be, because it comes from the parser that compiles the document.
Keeps the "no upstream code" line clean.

**Costs.** A visible flash of approximate coloring on file open before the first semantic-token response,
and degraded coloring whenever the server is down or disabled. Delta encoding matters here: a document
produces thousands of tokens and re-sending them per keystroke would be wasteful, so `full/delta` is not
optional.

**Not chosen.** Adopting tinymist's generated grammar would be faster and immediately more accurate for the
pre-server paint. It is Apache-2.0 and adoption would be legitimate with attribution. Rejected for now
because it imports a build step and a maintenance surface to solve a problem that lasts a few hundred
milliseconds per file open.

## Revisit if

- The pre-semantic-token flash draws complaints, or the minimal grammar is visibly wrong often enough to
  be noticed.
- Users run with `semanticTokens: "disable"` in numbers, making the fallback the common path rather than
  the degraded one.
- Maintaining even a minimal grammar across typst syntax additions turns out to cost more than adopting
  tinymist's generator would have.
