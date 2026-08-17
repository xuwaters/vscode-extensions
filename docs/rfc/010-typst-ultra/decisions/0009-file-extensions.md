# 0009 — Claim `.typ` and `.typc` under one language id

**Status**: Accepted
**Date**: 2026-08-17
**Resolves**: OQ 8

## Context

Typst tooling has a convention that `.typ` is markup mode and `.typc` is "code mode" — a file that is all
code, no markup. Tinymist ships two language ids for this: `typst` (`.typ`) and `typst-code` (`.typc`),
with different grammars.

Before mirroring that, the prototype tested what the **compiler** actually does with a `.typc` file
([research/spike.md §8](../research/spike.md#8-typc-and-code-mode)):

```
import .typc written in CODE mode:    err ["unresolved import"]
import .typc written in MARKUP mode:  ok pages=1
```

The result is unambiguous: **upstream typst parses every imported file as markup, regardless of extension.**
A `.typc` file written in genuine code mode (`let x = 1`) fails to import. One written in markup mode
(`#let x = 1`) imports fine. `typst-syntax` does expose `parse_code`, but `Source` is always constructed
markup-first, and `Source::edit`'s non-incremental fallback path unconditionally calls `parse(text)` — so
even a code-mode tree would silently revert to markup on the first non-incremental edit.

`.typc` is therefore an **editor convention that the compiler does not honour**.

## Decision

Claim both extensions under a **single language id, `typst`**:

```jsonc
"languages": [{
  "id": "typst",
  "extensions": [".typ", ".typc"],
  "aliases": ["Typst", "typst"],
  "configuration": "./language-configuration.json"
}]
```

Both are parsed as **markup**, exactly as the compiler does. We do not use `parse_code`, and we do not ship
a separate `typst-code` language.

The extension README states plainly that typst compiles `.typc` as markup, so a `.typc` file needs `#let`
rather than bare `let` — this is upstream behaviour, not our limitation, and users arriving from tinymist
are the ones who will hit it.

## Consequences

**Buys.** `.typc` files get full editor support — grammar, semantic tokens, completion, diagnostics,
formatting — at essentially zero cost, since they are handled identically to `.typ`. One grammar, one
language-configuration, one code path.

**Correctness over convention.** The editor agrees with the compiler. Highlighting a `.typc` file in code
mode would look nicer *and be wrong*: it would color `let x = 1` as valid code while the compiler rejects
it. Disagreeing with the compiler is a worse failure than plain markup highlighting.

**Costs.** A user with genuinely code-mode `.typc` files (a tinymist convention they may have adopted)
sees markup highlighting and gets compiler errors. The README explains why; we cannot fix it without the
compiler changing.

## Revisit if

- Upstream typst starts honouring `.typc` as code mode — at which point `parse_code` plus a `typst-code`
  language id becomes correct rather than misleading, and this record should be superseded.
- `typst-syntax` gains a way to build a `Source` that durably retains its parse mode across
  `Source::edit`, which is the technical blocker behind the above.
- `.typm` (math mode) becomes a convention worth claiming; the same test should be run before assuming.
