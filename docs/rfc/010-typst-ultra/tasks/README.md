# Tasks

Progress tracking for RFC 010. One file per phase; phases are defined in
[proposal.md §12](../proposal.md#12-phased-plan).

## Board

| Phase | Goal | Exit criterion | Status |
| --- | --- | --- | --- |
| [1 — Foundation](phase-1-foundation.md) | Server skeleton, VFS, fonts, diagnostics | Open a `.typ`, see typst's real errors as you type | ☐ Not started |
| [2 — IDE features](phase-2-ide-features.md) | The full LSP surface, packages, compile root | `#import "@preview/cetz:0.4.2"` completes, resolves, jumps | ☐ Not started |
| [3 — Preview](phase-3-preview.md) | Live preview, two-way sync, export | Type → repaint < 120 ms; click a page → land on the source | ☐ Not started |
| [4 — Polish](phase-4-polish.md) | Hints, actions, PNG mode, real-world validation | Each item independently shippable | ☐ Not started |

**Overall: 0 / 62 tasks complete.** RFC is in review; no implementation has started.

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

Measurements the RFC currently relies on but has not made. Each is owned by a task, so they get closed
rather than quietly inherited. Source: [research/spike.md §10](../research/spike.md#10-what-the-spike-did-not-cover).

| Debt | Why it matters | Closed by |
| --- | --- | --- |
| Transport latency (JSON-RPC + IPC + postMessage + DOM parse) is **estimated**, ~50 ms of the 65 ms preview budget | If wrong, [0006](../decisions/0006-preview-rendering.md) escape hatches are needed in Phase 3, not Phase 4 | [P3-05](phase-3-preview.md) |
| No real-world documents benchmarked — synthetic `#lorem` only, no images, CeTZ, bibliographies, or large tables | Every latency and SVG-size number in the RFC could be optimistic | [P4-08](phase-4-polish.md) |
| System-font indexing on a large font directory never measured | First-run cost on a designer's machine is unknown | [P2-13](phase-2-ide-features.md) |
| Package download, extraction, and caching unimplemented and unmeasured | Whole feature is unproven | [P2-14](phase-2-ide-features.md) |
| Only measured on aarch64 macOS | WASM makes parity likely, not certain | [P4-09](phase-4-polish.md) |
| Multi-file compile graphs proven functional but not benchmarked | Diagnostics fan-out cost is unknown | [P1-16](phase-1-foundation.md) |
| Font parsing cost at startup folded into a cold-start number, never isolated | Affects the "server start < 400 ms" target | [P1-11](phase-1-foundation.md) |

## Continuous

Not phase-bound; ongoing for the life of the extension.

| Item | Cadence | Notes |
| --- | --- | --- |
| Track upstream typst releases | Per release | Version bump + API drift. The whole point of [0001](../decisions/0001-unmodified-upstream-typst.md) is that this stays cheap; if it stops being cheap, that record needs revisiting |
| Re-run the eviction sweep | On comemo upgrade | [0005](../decisions/0005-cache-eviction-policy.md) depends on comemo's current semantics |
| Regenerate third-party notices | On dependency change | `cargo-about`, output committed so the diff is reviewable |
