# Phase 6 — BibTeX bibliographies

**Goal**: a `.bib` file is a first-class part of a typst project rather than an opaque blob the compiler
reads. Diagnostics, an outline, colouring, completion, and citations that resolve in both directions.
**Exit criterion**: open `refs.bib` and a duplicate key is underlined where it is written; open the paper
that cites it and `@knuth1984` hovers, jumps to the entry, and renames across both files at once.
**Status**: ☑ Complete — 9 / 9

Scope comes from [0011](../decisions/0011-bibtex-support.md), which also records why neither `biblatex` nor
`hayagriva` could be the parser and why this is one server rather than two. The surface is
[lsp-features.md §8](../design/lsp-features.md#8-bibtex-bibliographies-phase-6).

## Tasks

### The parser

| ID | Task | Spec | Status |
| --- | --- | --- | --- |
| P6-01 | `typst-lsp-core/src/bib.rs`: entries, keys, fields, and values with byte ranges on every node; both BibTeX and biblatex dialects; `{…}` and `(…)` delimiters; `@string` concatenation expanded in a second pass; brace-protected capitalisation and LaTeX escapes cleaned for display only | [0011 A](../decisions/0011-bibtex-support.md) | ☑ |
| P6-02 | Error recovery: an unclosed entry stops at the next `@` that starts a line, so a file being typed keeps working below the cursor. A token stream that covers the gaps too — text outside an entry is a comment in BibTeX and is coloured as one | [0011 A](../decisions/0011-bibtex-support.md) | ☑ |
| P6-03 | Lints: duplicate citation keys and duplicate fields (error, with the first occurrence as related information); missing required fields and unknown entry types (warning). `year` **or** `date` satisfies one requirement, so a biblatex file and a BibTeX file are both complete | [lsp-features.md §8.1](../design/lsp-features.md#81-routing) | ☑ |
| P6-04 | The canonical formatter: one field per line, trailing commas, one blank line between entries, values copied verbatim. Refuses on syntax errors, as the typst formatter does; idempotent, and tested for it | [lsp-features.md §8.1](../design/lsp-features.md#81-routing) | ☑ |

**P6-01 notes.** The parser is deliberately not a renderer. `{\"o}` stays `{\"o}` in every edit it
produces, and is only flattened for display; the moment a tooltip started *interpreting* LaTeX it would
begin disagreeing with the compiled document.

### The three wrongnesses

| ID | Task | Spec | Status |
| --- | --- | --- | --- |
| P6-05 | A `.bib` can no longer become the compile root — not by being focused (`did_open`, `compile_now`), not by `typst/setMain`, and not by being the document that started the server (`client.ts`'s `mainPath`) | [0011](../decisions/0011-bibtex-support.md) | ☑ |
| P6-06 | Routing: every handler asks `Server::bib_of(uri)` first. `formatting` no longer hands a bibliography to `typstyle`; symbols, folding, selection ranges, and semantic tokens come from the BibTeX parse; code actions, code lenses, inlay hints, and signature help decline | [lsp-features.md §8.1](../design/lsp-features.md#81-routing) | ☑ |
| P6-07 | Diagnostics on the edit rather than the compile, published and cleared through their own URI set so the two publishers cannot clear each other | [lsp-features.md §8.2](../design/lsp-features.md#82-two-clocks-again) | ☑ |

### Citations

| ID | Task | Spec | Status |
| --- | --- | --- | --- |
| P6-08 | In a `.bib`: hover, entries as document and workspace symbols, entry-type skeletons and field-name completion, `crossref` and `@string` goto-definition, `url`/`doi` links | [lsp-features.md §8.1](../design/lsp-features.md#81-routing) | ☑ |
| P6-09 | In a `.typ`: hover, goto-definition, and completion for `@key` fall back to the project's bibliographies when `typst-ide` has nothing; rename spans both languages | [lsp-features.md §8.1](../design/lsp-features.md#81-routing) | ☑ |

**P6-09 notes.** The fallback is not a nicety. `typst-ide` can only see the citation keys of the *last
compiled document*, so before the first compile — and in the far more common case where `bibliography()`
has not been written yet — it has nothing to offer, which is exactly when someone is reaching for a key.

## Extension

No new settings and no new commands. The extension contributes the `bibtex` language configuration VSCode's
built-in LaTeX extension does not have (brackets, auto-closing, comment toggle, and a word pattern that
keeps `knuth-1984:tlp` in one piece), adds `bibtex` to the document selector and the file watcher, and puts
`.bib` files in the `typst/workspaceFiles` list so a bibliography the compile has not read is still one the
reader can cite from.

Opening a `.bib` activates the extension, but it refuses to start the engine unless the workspace holds
typst files — a LaTeX project's bibliography costs an activation event and nothing else.

No TextMate grammar. VSCode already ships one for `bibtex`; semantic tokens do the real colouring, which is
[0007](../decisions/0007-textmate-grammar.md) applied unchanged.

## Tests

30 unit tests — 26 on the parser (dialects, recovery, lints, formatting idempotence, multibyte ranges, and
every prefix of a file, which is the file as it is typed) and four on the routing helpers — plus 20
feature tests through the same `on_request` / `on_notification` path the WASM binding uses. Two of those pin the compile interaction: an unsaved `.bib` edit reaching the
compiler, and a compile leaving the bibliography's own diagnostics standing.

One more runs through the **real artifact**, in `server/engine.test.ts`: a bibliography opened next to a
paper that cites it, compiled with the `.bib` as the debounce subject (the case that would have handed
BibTeX to the typst compiler), then broken to prove the parser's diagnostics come back across the WASM
boundary.
