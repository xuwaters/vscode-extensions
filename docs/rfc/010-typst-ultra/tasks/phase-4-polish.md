# Phase 4 — Polish and stretch

**Goal**: the features that are genuinely useful but not load-bearing, plus closing the RFC's remaining
research debts.
**Exit criterion**: none — every item here is independently shippable and independently droppable.
**Status**: ☐ Not started — 0 / 16

Unlike Phases 1–3, this list is a menu, not a sequence. Items are ordered by expected value.

## Validation — do these first

These close [research debts](README.md#research-debts) that the whole RFC currently rests on. If P4-08
finds that real documents behave nothing like the synthetic corpus, several earlier decisions need
revisiting — better to learn that early.

| ID | Task | Spec | Status |
| --- | --- | --- | --- |
| P4-08 | Real-world benchmark corpus: a paper with images + bibliography + tables, a CeTZ/Fletcher-heavy document, a 200-page book, a presentation. Re-run compile latency, SVG page size, and heap. Update [research/spike.md](../research/spike.md) and any decision the results contradict | [spike.md §10](../research/spike.md#10-what-the-spike-did-not-cover) | ☐ |
| P4-09 | Verify on Windows and Linux: font discovery paths, package cache directory, path separators in the VFS, child-process startup | [spike.md §10](../research/spike.md#10-what-the-spike-did-not-cover) | ☐ |
| P4-10 | Re-run the eviction sweep against the real-world corpus; confirm or revise the `evictAge: 1` default | [0005](../decisions/0005-cache-eviction-policy.md) | ☐ |

## Optimizations

| ID | Task | Spec | Status |
| --- | --- | --- | --- |
| P4-05 | **PNG low-memory render mode**: `typstUltra.preview.renderMode` = `svg` \| `png` \| `auto`, with `auto` switching per page above a ~1 MB SVG threshold. Re-render on zoom-step change; document the loss of find-in-preview | [0006](../decisions/0006-preview-rendering.md) | ☐ |
| P4-11 | SVG coordinate rounding to 2 decimal places — targets the 88% of page bytes that are `<use>` positioning. Cheaper than P4-05 and worth trying first | [0006](../decisions/0006-preview-rendering.md) | ☐ |
| P4-12 | Server heap watchdog: surface `memory.restartThresholdMb` in the status bar with a one-click restart | [architecture.md §6](../design/architecture.md#6-memory-and-the-comemo-cache) | ☐ |

## Language features

| ID | Task | Spec | Status |
| --- | --- | --- | --- |
| P4-01 | Inlay hints: parameter names at call sites from `Func` metadata; default off | [lsp-features.md §5](../design/lsp-features.md#5-phase-4-features) | ☐ |
| P4-02 | Signature help from `Func` params — declared parameters, types, and defaults only; **no evaluated argument values**, which would need the fork we are not doing | [lsp-features.md §5](../design/lsp-features.md#5-phase-4-features), [0001](../decisions/0001-unmodified-upstream-typst.md) | ☐ |
| P4-03 | Code actions: add missing import, wrap in `#{…}`, string → content block, add `<label>` to a heading | [lsp-features.md §5](../design/lsp-features.md#5-phase-4-features) | ☐ |
| P4-04 | Code lenses: "Preview" and "Export as…" above the first line | [lsp-features.md §5](../design/lsp-features.md#5-phase-4-features) | ☐ |
| P4-13 | Postfix / UFCS completions (`x.rect` → `rect(x)`) — a pure syntax feature, rebuilding what tinymist offers | [lsp-features.md §3.1](../design/lsp-features.md#31-completion) | ☐ |

## Stretch

| ID | Task | Spec | Status |
| --- | --- | --- | --- |
| P4-06 | Browser-worker build for vscode.dev: `vscode-languageclient/browser` + `Worker`, `wasm-pack --target web`. Needs a VFS over `vscode.workspace.fs` (async) — a real redesign of the synchronous host services, so scope it before starting | [0003](../decisions/0003-server-in-child-process.md) | ☐ |
| P4-07 | HTML export via `typst-html` — already linked in transitively, so the cost is UX, not dependencies | [proposal.md §10](../proposal.md#10-known-limitations-from-the-no-fork-constraint) | ☐ |
| P4-14 | Template / `init` support: scaffold a project from a Universe template package | — | ☐ |
| P4-15 | Adopt or generate a full TextMate grammar, if [0007](../decisions/0007-textmate-grammar.md)'s revisit conditions are met | [0007](../decisions/0007-textmate-grammar.md) | ☐ |
| P4-16 | Upstream contributions: propose exports for anything [0001](../decisions/0001-unmodified-upstream-typst.md)'s option 3 identified — the honest alternative to forking | [0001](../decisions/0001-unmodified-upstream-typst.md) | ☐ |

## Explicitly not planned

DAP debugging · coverage · profiling flamegraphs · `typst test` · symbol picker · font browser ·
`tinymist.lock`-style project resolution · LaTeX/Word import · editing from the preview.

These are [proposal.md §2](../proposal.md#2-goals-and-non-goals) non-goals. Listing them here so that
"why doesn't it do X?" has an answer in the place people will look.
