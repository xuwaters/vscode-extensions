# Tasks

Progress tracking for RFC 011. One file per phase; phases are defined in
[proposal.md §6](../proposal.md#6-phases).

## Board

| Phase | Goal | Exit criterion | Status |
| --- | --- | --- | --- |
| [1 — Foundation](phase-1-foundation.md) | Parser, WASM, plugin skeleton, packaging | An empty plugin carrying the WASM loads in a real tsserver from an installed VSIX, and the parser matches parse5 | ☑ 9 / 10 · ◐ 1 |
| [2 — Component model](phase-2-component-model.md) | Discovery, registry, HTML data, imports | All corpus elements found with their members, from `@customElement({ name: CONST })` | ☑ 12 / 12 |
| [3 — Diagnostics](phase-3-diagnostics.md) | 26 rules, the type oracle, config | Zero diagnostics over the corpus; a seeded mistake of each rule's kind is reported | ☑ 13 / 14 · ◐ 1 |
| [4 — IDE features](phase-4-ide-features.md) | The position features, CSS, colours, the command | `${ref('` completes; renaming a member updates its bindings | ☑ 15 / 17 · ◐ 2 |
| [5 — Polish and release](phase-5-polish.md) | Grammar, docs, licences, measurement | VSIX published, every performance claim measured | ☑ 6 / 10 · ◐ 3 · ☐ 1 |

**Overall: 55 / 63 done, 7 in progress with named remainders, 1 not started (P5-09).**
Every phase exit criterion is green in the test suite:
`extensions/fast-element-ultra` — 107 vitest tests across 8 files (corpus gates, seeded rule
fixtures, the parse5 differential, feature round-trips, panic containment) plus 89 Rust tests.

The in-progress remainders, in one list:

- **P1-09 / P5-06**: install the built VSIX into a clean desktop VS Code and read the plugin's name
  in the TS Server log (everything headless is automated and passing — `pnpm run verify:vsix`).
- **P3-11**: the `@attr({ converter })` `fromView` return-type check (converter presence currently
  silences the rule).
- **P4-08**: the didyoumean2 suggestion comparison, blocked on `temp/fast-analyzer` being absent.
- **P4-11**: cross-project references (within-project is complete).
- **P5-04**: the fast-analyzer side-by-side (same blocker as P4-08) and a 10× scaling curve.
- **P5-10**: marketplace publication, if this extension is ever published beyond the repo.

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

| Debt | Closed by | Outcome |
| --- | --- | --- |
| Can the plugin be packaged under `--no-dependencies`? **Blocking** | [P1-09](phase-1-foundation.md) | ☑ — via post-package injection; the negation route is structurally impossible in vsce. [decisions/README.md](../decisions/README.md#open-questions--all-closed) q1 |
| Does our parser match parse5? **Gates Phase 2** | [P1-05](phase-1-foundation.md) | ☑ — passed; divergences enumerated. q2 |
| Cold and warm diagnostic cost | [P5-04](phase-5-polish.md) | ☑ measured (0.73 ms warm) — fast-analyzer side-by-side still open (`temp/` absent). [research/measurements.md](../research/measurements.md) |
| WASM artifact size, instantiation, heap, growth over 1,000 edits | [P5-04](phase-5-polish.md) | ☑ — 588 KB / 1.7 ms / no growth |
| Binding-fact batch size and per-fact cost | [P3-09](phase-3-diagnostics.md) | ☑ — 32/13 and 60/11 docs on the corpus; inside the warm pass |
| Does `strsim` match `didyoumean2` on the fixtures? | [P4-08](phase-4-ide-features.md) | ◐ — pinned by our own fixtures; the comparison needs `temp/` restored |
| Which TypeScript versions does the plugin work against? | [P1-08](phase-1-foundation.md) | ☑ — >=5.5 <8 declared and gate-tested; exercised against TS 6 |

## Continuous

| Item | Cadence | Notes |
| --- | --- | --- |
| Track `@microsoft/fast-element` releases | Per release | [research/fast-element.md](../research/fast-element.md) is read from source and goes stale silently. A minor release that adds a directive or a decorator is a component-model change |
| Re-run the corpus gate | Every PR | `test/corpus.test.ts` + `test/parser-differential.test.ts`, in the extension's suite. It grows as the repo's own extensions grow |
| Regenerate the built-in HTML data tables | On `@vscode/web-custom-data` upgrade | `pnpm run generate:htmldata`; the generated file is committed so the diff is reviewable |
| Track TypeScript releases | Per release | The plugin API is not a stability contract ([0001](../decisions/0001-tsserver-plugin-not-lsp.md)); the version gate's ceiling (<8) is the reminder |
| Regenerate third-party notices | On dependency change | `pnpm run licenses` (cargo-about), plus LICENSE.md's hand-kept JS section |
