# Decision Records

One file per load-bearing decision. A decision record exists so that six months from now, someone who
disagrees with a choice can find out *why* it was made and *what would have to change* to reverse it —
without reading the whole RFC or digging through chat logs.

## Index

| # | Decision | Status | Resolves |
| --- | --- | --- | --- |
| [0001](0001-unmodified-upstream-typst.md) | Build on unmodified upstream typst; never fork | Accepted | — |
| [0002](0002-single-wasm-artifact.md) | Ship one WASM artifact, not per-platform binaries | Accepted | — |
| [0003](0003-server-in-child-process.md) | Run the server in a child process; LSP and preview share it | Accepted | OQ 3 |
| [0004](0004-bundle-default-fonts.md) | Bundle typst's default fonts as VSIX assets | Accepted, amended by 0012 | OQ 5 |
| [0005](0005-cache-eviction-policy.md) | Compile then evict; default eviction age `1` | Accepted | OQ 4 |
| [0006](0006-preview-rendering.md) | Per-page SVG preview; PNG as a low-memory mode | Accepted | OQ 2 |
| [0007](0007-textmate-grammar.md) | Minimal TextMate grammar; semantic tokens do the real work | Accepted | OQ 1 |
| [0008](0008-compile-root.md) | Follow the focused file by default; pin a main file for projects | Accepted | OQ 6 |
| [0009](0009-file-extensions.md) | Claim `.typ` and `.typc` under one language id | Accepted | OQ 8 |
| [0010](0010-naming.md) | `typst-ultra`, four crates under `crates/typst/` | Accepted | OQ 7 |
| [0011](0011-bibtex-support.md) | Speak BibTeX in the same server, with our own parser | Accepted | — |
| [0012](0012-fonts-in-a-companion-extension.md) | Ship the bundled fonts in a companion extension | Accepted | — |

"OQ n" refers to the numbered open questions in [proposal.md §11](../proposal.md#11-open-questions), which
now points here rather than restating the answers.

## Statuses

| Status | Meaning |
| --- | --- |
| **Proposed** | Written down, not yet agreed |
| **Accepted** | Agreed and in force. Implementation should match it |
| **Superseded by NNNN** | Replaced. Keep the file; add the pointer at the top |
| **Reversed** | Tried and abandoned. The *why* is the valuable part — do not delete |

## Adding one

Copy the shape of any existing record:

```markdown
# NNNN — <short imperative title>

**Status**: Proposed | Accepted | Superseded by NNNN | Reversed
**Date**: YYYY-MM-DD
**Resolves**: OQ n (optional)

## Context
What forced a choice. Include measurements where they exist — link to research/, don't re-derive.

## Decision
What we are doing, stated so an implementer can act on it without interpretation.

## Consequences
What this costs, what it makes easy, what it makes hard. Be honest about the downsides.

## Revisit if
The specific, observable conditions that would make this decision wrong.
```

Rules that keep this useful rather than ceremonial:

- **Records are append-only in spirit.** Correct typos freely; change a decision by adding a new record
  that supersedes the old one, not by editing history.
- **"Revisit if" is mandatory.** A decision with no falsifying condition is a preference, not a decision.
- **Numbers belong in [research/](../research/), not here.** Records link to measurements; they do not
  restate them, so there is exactly one place to update when a measurement is redone.
- **An outcome is not a new decision.** When implementation confirms or contradicts what a record assumed,
  add an `## Outcome` section to that record rather than superseding it — the decision did not change, the
  evidence did. Three records now carry one:
  [0003](0003-server-in-child-process.md) (blocking is worse than measured, which strengthens it),
  [0005](0005-cache-eviction-policy.md) (age 1 confirmed, by narrower margins),
  [0006](0006-preview-rendering.md) (transport 8× cheaper; escape hatch 1 nearly worthless).
