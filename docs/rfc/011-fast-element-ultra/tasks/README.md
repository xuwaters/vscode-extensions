# Tasks

Progress tracking for RFC 011. One file per phase; phases are defined in
[proposal.md §6](../proposal.md#6-phases).

## Board

| Phase | Goal | Exit criterion | Status |
| --- | --- | --- | --- |
| [1 — Foundation](phase-1-foundation.md) | Parser, WASM, plugin skeleton, packaging | An empty plugin carrying the WASM loads in a real tsserver from an installed VSIX, and the parser matches parse5 | ☐ 0 / 10 |
| [2 — Component model](phase-2-component-model.md) | Discovery, registry, HTML data, imports | All 5 corpus elements found with their members, from `@customElement({ name: CONST })` | ☐ 0 / 12 |
| [3 — Diagnostics](phase-3-diagnostics.md) | 26 rules, the type oracle, config | Zero diagnostics over the corpus; a seeded mistake of each rule's kind is reported | ☐ 0 / 14 |
| [4 — IDE features](phase-4-ide-features.md) | The ten position features, CSS, colours, the command | `${ref('` completes; renaming a member updates its bindings | ☐ 0 / 17 |
| [5 — Polish and release](phase-5-polish.md) | Grammar, docs, licences, measurement | VSIX published, and every performance claim in the RFC replaced with a measured one | ☐ 0 / 10 |

**Overall: 0 / 63.** Nothing started.

Phase 1 is front-loaded with the two things that can invalidate the design —
[gate 1](../research/spikes.md#gate-1--can-the-plugin-be-packaged-at-all) (P1-09) and
[gate 2](../research/spikes.md#gate-2--does-our-parser-match-parse5) (P1-05) — rather than with the
work that is most fun to start. Neither is a coding task; both are questions with a yes-or-no answer,
and a no changes the plan.

## Status legend

| Mark | Meaning |
| --- | --- |
| ☐ | Not started |
| ◐ | In progress |
| ☑ | Done — merged, tested |
| ⊘ | Dropped — with a one-line reason |
| ⊗ | Blocked — name the blocker |

## Conventions

Carried from [RFC 010](../../010-typst-ultra/tasks/README.md), because they worked:

- **Task IDs are stable.** `P2-07` keeps its number even if it is dropped or reordered; the number is
  how commits, PRs and decision records refer to it. Never renumber.
- **A task is done when it is merged with its tests**, not when the code exists. Phase files name the
  test for each task where one is expected.
- **Update the phase file in the same PR as the work.**
- **Tasks do not restate design.** They link to the design document or decision record that specifies
  them. If a task needs a decision that does not exist, writing the record is itself the task.

## Research debts

Everything this RFC asserts about performance. All of it is open, because none of it has been
measured — see [research/spikes.md](../research/spikes.md).

| Debt | Closed by | Outcome |
| --- | --- | --- |
| Can the plugin be packaged under `--no-dependencies`? **Blocking** | [P1-09](phase-1-foundation.md) | ☐ |
| Does our parser match parse5? **Gates Phase 2** | [P1-05](phase-1-foundation.md) | ☐ |
| Cold and warm diagnostic cost, ours and fast-analyzer's on the same input | [P5-04](phase-5-polish.md) | ☐ |
| WASM artifact size, instantiation cost, resident heap, growth over 1,000 edits | [P5-04](phase-5-polish.md) | ☐ |
| Binding-fact batch size per file, and the cost of answering one | [P3-09](phase-3-diagnostics.md) | ☐ |
| Does `strsim` match `didyoumean2`'s suggestions on the fixtures? | [P4-08](phase-4-ide-features.md) | ☐ |
| Which TypeScript versions does the plugin actually work against? | [P1-08](phase-1-foundation.md) | ☐ |

Until each of these is closed, **no number derived from them appears in any document in this RFC**.
The [README](../README.md#what-is-already-established-and-what-is-not) says so explicitly, and it is
the rule most likely to be broken by accident.

## Continuous

| Item | Cadence | Notes |
| --- | --- | --- |
| Track `@microsoft/fast-element` releases | Per release | [research/fast-element.md](../research/fast-element.md) is read from source and goes stale silently. A minor release that adds a directive or a decorator is a component-model change |
| Re-run the corpus gate | Every PR | [research/corpus.md §4](../research/corpus.md#4-how-the-corpus-is-used). It grows as the repo's own extensions grow |
| Regenerate the built-in HTML data tables | On `vscode-html-languageservice` / `@vscode/web-custom-data` upgrade | Generated file is committed so the diff is reviewable — same arrangement as typst-ultra's grammar |
| Track TypeScript releases | Per release | The plugin API is not a stability contract ([0001](../decisions/0001-tsserver-plugin-not-lsp.md)) |
| Regenerate third-party notices | On dependency change | `cargo about`, plus the JavaScript dependencies the plugin bundles |
