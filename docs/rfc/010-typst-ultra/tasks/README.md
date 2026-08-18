# Tasks

Progress tracking for RFC 010. One file per phase; phases are defined in
[proposal.md §12](../proposal.md#12-phased-plan).

## Board

| Phase | Goal | Exit criterion | Status |
| --- | --- | --- | --- |
| [1 — Foundation](phase-1-foundation.md) | Server skeleton, VFS, fonts, diagnostics | Open a `.typ`, see typst's real errors as you type | ☑ 16 / 16 |
| [2 — IDE features](phase-2-ide-features.md) | The full LSP surface, packages, compile root | `#import "@preview/cetz:0.4.2"` completes, resolves, jumps | ☑ 17 / 17 |
| [3 — Preview](phase-3-preview.md) | Live preview, two-way sync, export | Type → repaint < 120 ms; click a page → land on the source | ☑ 13 / 13 |
| [4 — Polish](phase-4-polish.md) | Hints, actions, PNG mode, real-world validation | Each item independently shippable | ◐ 14 / 16 |

**Overall: 60 / 62 complete**, 1 scoped ([P4-06](phase-4-polish.md), browser build — blocked on a
question about vscode.dev), 1 blocked ([P4-09](phase-4-polish.md), Windows/Linux verification — needs
those machines).

### Where it stands

Four Rust crates and one extension. **751 Rust tests and 91 extension tests pass**; the WASM artifact is
**26.5 MB raw / 10 MB gzipped**, and the VSIX is **16 MB**. The whole stack is exercised end to end
through the real artifact by `server/engine.test.ts` — multi-file compile with synchronous host callbacks
firing mid-compile, hover, page rendering with hash diffing, PDF export, and diagnostics appearing and
clearing — and through the **built server over a real LSP connection** by `server/lsp.test.ts`, which
forks `dist/server.js` the way `vscode-languageclient` does and speaks JSON-RPC to it.

## Status legend

| Mark | Meaning |
| --- | --- |
| ☐ | Not started |
| ◐ | In progress |
| ☑ | Done — merged, tested |
| ⊘ | Dropped — with a one-line reason |
| ⊗ | Blocked — name the blocker |

## Conventions

- **Task IDs are stable.** `P2-07` keeps its number even if it is dropped or reordered; the number is how
  commits, PRs, and decision records refer to it. Never renumber.
- **A task is done when it is merged with its tests**, not when the code exists. Phase files name the test
  for each task where one is expected.
- **Update the phase file in the same PR as the work.** A tracking file that lags the code is worse than
  no tracking file.
- **Tasks do not restate design.** They link to the design doc or decision record that specifies them. If a
  task needs a decision that does not exist yet, that is itself a task — write the record first.

## Research debts

Measurements the RFC relied on but had not made. Source:
[research/spike.md §10](../research/spike.md#10-what-the-spike-did-not-cover).

| Debt | Closed by | Outcome |
| --- | --- | --- |
| Transport latency **estimated** at ~50 ms of the 65 ms preview budget | [P3-05](phase-3-preview.md) | ☑ **3.7 ms** — 8× cheaper than estimated. [transport.md](../research/transport.md) |
| No real-world documents benchmarked — synthetic `#lorem` only | [P4-08](phase-4-polish.md) | ☑ Four documents, native and WASM. Page size +22%, book cold compile misses its target. [corpus.md](../research/corpus.md) |
| System-font indexing on a large font directory never measured | [P2-13](phase-2-ide-features.md) | ◐ Machinery tested and cached; **a font-heavy machine was not available** |
| Package download, extraction, and caching unimplemented and unmeasured | [P2-14](phase-2-ide-features.md) | ◐ Implemented and tested, including traversal refusal; **a live registry download is unverified** |
| Only measured on aarch64 macOS | [P4-09](phase-4-polish.md) | ⊗ Platform branches unit-tested from any host; **runtime verification still needs the machines** |
| Multi-file compile graphs proven functional but not benchmarked | [P1-16](phase-1-foundation.md) | ☑ Fan-out is not separately expensive — cost tracks content, not file count |
| Font parsing cost at startup folded into a cold-start number | [P1-11](phase-1-foundation.md) | ☑ **10 ms cold, <1 ms cached** — 2.5% of the 400 ms budget |

Three debts remain open, and all three need something this environment does not have: a font-heavy
machine, a route to `packages.typst.org`, and Windows/Linux hosts. Each is recorded where it will be
looked for rather than quietly inherited.

### New debts, opened by the implementation

| Debt | Why it matters | Owner |
| --- | --- | --- |
| **DOM parse and paint** for a 470 KB page is still an estimate | It is the only remaining row in the preview budget carrying a guess. `happy-dom` is not a browser and does no paint | [P4-08](phase-4-polish.md), when a webview can be instrumented |
| **Raster images** are absent from the corpus | Every figure in it is vector, and vector turned out to be cheap. Photographs would test a different path entirely | [P4-08](phase-4-polish.md) |
| **Cold compile misses its target** on a long structured book (524 ms vs a 300 ms/75-page target) | The target should be restated per page (< 6 ms/page); changing a stated target is a scope decision | An amendment to [proposal.md §8](../proposal.md#8-performance-targets) |

## Continuous

Not phase-bound; ongoing for the life of the extension.

| Item | Cadence | Notes |
| --- | --- | --- |
| Track upstream typst releases | Per release | Version bump + API drift. The whole point of [0001](../decisions/0001-unmodified-upstream-typst.md) is that this stays cheap; if it stops being cheap, that record needs revisiting |
| Re-run the eviction sweep | On comemo upgrade | [0005](../decisions/0005-cache-eviction-policy.md) depends on comemo's current semantics. `cargo run -p typst-session --example corpus` |
| Regenerate third-party notices | On dependency change | `pnpm run licenses` (cargo-about), output committed so the diff is reviewable |
| Re-copy the typst-assets `NOTICE` | On typst-assets upgrade | `LICENSE.md` §4 is verbatim upstream. `dump-fonts` asserts the font set has not changed underneath it |
