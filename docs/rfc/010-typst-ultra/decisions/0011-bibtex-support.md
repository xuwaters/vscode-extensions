# 0011 — Speak BibTeX in the same server, with our own parser

**Status**: Accepted
**Date**: 2026-08-17

## Context

A typst document with a bibliography is a document plus a `.bib` file. `bibliography("refs.bib")` reads
it, `@knuth1984` cites into it, and the whole thing compiles as one unit. Until now the server saw only
half of that: the `.bib` was bytes the compiler read, and in the editor it was a text file with no
diagnostics, no outline, no colouring beyond VSCode's built-in grammar, and no connection to the citations
pointing at it.

Worse than absent — actively wrong in three places, all of which fall out of `.bib` documents being stored
in the same `Source` overlay every other file is:

1. **The compile root followed the focused editor**, so opening a `.bib` made *it* the file typst was asked
   to compile.
2. **`textDocument/formatting` ran `typstyle`**, which would have rewritten the file as typst markup.
3. **Symbols, folding, and semantic tokens came from typst's parser**, which produces nonsense on BibTeX.

The reading half was already right and worth keeping: `Vfs::file` prefers the overlay, so an unsaved edit
to a `.bib` already reached the compiler. A test now pins that
([`an_unsaved_bibliography_edit_reaches_the_compiler`](../../../../crates/typst/typst-lsp-core/tests/features.rs)).

Two upstream parsers were candidates for the reading. `biblatex` is already in the graph — typst compiles
bibliographies through it — and `hayagriva` sits above it. Neither answers the question an editor asks.
They answer "does this file parse, and what does it mean"; an IDE needs "what is at byte 412, where does
the entry under the cursor start and end, and what are the ranges of every key, field name, and value".
`biblatex::Entry` carries no span for its own citation key, and both parsers stop at the first thing they
cannot make sense of — which, in a file being typed, is most of the time.

## Decision

**BibTeX is part of the language server**, not a second extension and not a client-side feature. Three
parts:

**A. Our own forgiving parser**, `typst-lsp-core/src/bib.rs` (~600 lines, 28 tests). Byte ranges on every
node; recovery at the next `@` that starts a line, so an unclosed entry costs one entry rather than the
rest of the file; a token stream that covers the gaps as well as the entries, because text outside an entry
is a comment in BibTeX and colouring it as one is how a reader sees it is being ignored. It reads both
dialects (BibTeX and biblatex) because typst does.

**B. Routing rather than a parallel server.** Every handler asks `Server::bib_of(uri)` first and takes the
BibTeX path when the answer is `Some`; the dispatch table, the document overlay, the URI map, and the token
cache are all unchanged. `.bib` files are excluded from the compile root (`did_open`, `compile_now`,
`typst/setMain`), and the four typst-only features — code actions, code lenses, inlay hints, signature help
— decline rather than answering nonsense.

**C. Citations resolve in both directions.** In a `.bib`: hover, entries as symbols, folding, entry-type and
field-name completion with skeletons, `crossref` and `@string` goto-definition, `url`/`doi` links, and a
canonical formatter. In a `.typ`: hover, goto-definition, and completion for `@key` fall back to the
project's bibliographies when `typst-ide` has nothing — which is *before the first compile*, and whenever
`bibliography()` has not been written yet. Rename spans both: one edit rewrites the entry and every
citation.

**Bibliography diagnostics run on the edit, not on the compile.** Parsing is instant, and a `.bib` file no
`bibliography()` call names would otherwise never be checked at all. They are published and cleared through
their own set (`bib_published`), because the compile's clear-the-difference pass would otherwise wipe them
on its way past — the two publishers are on different clocks and neither may clear the other.

**No grammar is shipped.** VSCode's built-in LaTeX extension already contributes the `bibtex` language id
and a `text.bibtex` grammar; what it does *not* contribute is a language configuration, so that is what the
extension adds (brackets, auto-closing, comment toggle, a word pattern that keeps `knuth-1984:tlp` in one
piece). Semantic tokens do the real colouring, exactly as [0007](0007-textmate-grammar.md) says for typst.

## Consequences

**Buys.** A bibliography stops being a blind spot: duplicate keys are an error where they are written,
incomplete entries are a warning, and a mistyped citation is one click from the entry it should have named.
The three wrongnesses above are fixed by construction rather than by warning people off. Editing a `.bib`
recompiles the document that cites it instead of trying to compile the bibliography.

**Costs.** A hand-written parser is a hand-written parser: BibTeX has corners (`@string` concatenation,
`(…)` delimiters, brace-protected capitalisation, LaTeX escapes) and each one is ours to get right. The
parser handles all four; the tests name them. It is deliberately *not* a renderer — it never expands a
`\LaTeX` command or a `{\"o}` into anything, because the moment it did, a tooltip would start disagreeing
with the compiled document.

**Costs.** Two token-legend entries were added (`property`, `variable`). Both are standard LSP types every
theme already colours, so no scope mapping was needed, but the legend is a wire contract and appending to
it is the only safe way to change it.

**Costs.** Opening a `.bib` now activates the extension. It refuses to start the engine unless the
workspace actually holds typst files, so a LaTeX project's bibliography costs an activation event and
nothing else.

**Not chosen.** Depending on `biblatex` for the parse and keeping ranges separately — two parsers over one
file, disagreeing at exactly the moments an editor is most useful. Also not chosen: a separate `.bib`
language server, which would need its own process, its own copy of the workspace, and a protocol between
the two to answer "which keys does this project define".

## Revisit if

- `biblatex` grows spans for keys and entry types **and** error recovery. Then the parser is upstream's
  problem and this one should be deleted, not maintained.
- Hayagriva's `.yml` bibliographies — typst's other bibliography format, already supported by the
  compiler — become common enough to want the same treatment. The routing in **B** is format-agnostic; only
  the parser in **A** is BibTeX-specific.
- Citation *styles* come up. Rendering `@knuth1984` the way the compiled document renders it needs CSL, and
  that is a different project than reading a `.bib` file.
