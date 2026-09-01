# Tasks — RFC 012

**This folder is the source of truth for progress.** One file per phase; phases are
defined in [proposal.md §5](../proposal.md#5-phases). Whoever does the work updates the
phase file *in the same change*.

## Board

| Phase | Goal | Exit criterion | Status |
| --- | --- | --- | --- |
| [1 — Spec pipeline](phase-1-spec-pipeline.md) | `glsl-spec` + `glsl-spec-gen`, generated tables committed | `mix`/`texture`/`gl_FragCoord` correct from embedded tables; regeneration deterministic | ☐ 0 / 10 |
| [2 — Lexer & preprocessor](phase-2-lexer-preprocessor.md) | Tokens, directives, macro expansion with provenance | Corpus lexes+preprocesses, zero panics; fixtures green | ☐ 0 / 7 |
| [3 — Parser & CST](phase-3-parser.md) | Lossless CST, full grammar, recovery | Corpus parses, zero panics; outline parity on `examples/` | ☐ 0 / 8 |
| [4 — Semantic analysis](phase-4-semantics.md) | Scopes, types, overloads, diagnostics | Every diagnostic has a seeded fixture; false-positive gate green | ☐ 0 / 10 |
| [5 — LSP integration](phase-5-lsp-integration.md) | GLSL routed to the new pipeline, all features | Feature tests green for ES/desktop/Vulkan; wasm budget held | ☐ 0 / 10 |
| [6 — Polish & release](phase-6-release.md) | Docs, notices, measurements, VSIX | Packaged VSIX; measurements recorded | ☐ 0 / 6 |

**Dependencies:** 1 ∥ 2 (parallel, disjoint crates) → 3 needs 2 → 4 needs 3 + 1 →
5 needs 4 → 6 needs 5.

## Status legend

| Mark | Meaning |
| --- | --- |
| ☐ | Not started |
| ◐ | In progress — name what remains |
| ☑ | Done — code merged **with its tests green** |
| ⊘ | Dropped — one-line reason stays in the row |
| ⊗ | Blocked — name the blocker |

## Conventions (carried from RFC 010/011, unchanged)

- **Task IDs are stable.** `P2-03` keeps its number forever; never renumber.
- **A task is done when its tests are green**, not when the code exists. Each task row
  names its test where one is expected.
- **Update the phase file in the same change as the work**, with a one-line note in the
  Notes column (what landed, or what remains for ◐).
- **Tasks do not restate design.** They link to the design doc or decision record. If a
  task needs a decision that does not exist, writing the record *is* the task.
- **Execution ground rules** for agents are in the [RFC README](../README.md) — no
  copying from `temp/`, no `cargo fmt`, tests in-tree, corpus read in place.

## Research debts

| Debt | Closed by | Outcome |
| --- | --- | --- |
| docs.gl page inventory: uniformity, ES version encoding, per-page exclusions. **Gates the generator design.** | P1-01 | — |
| docs.gl / Khronos refpage license and required attribution (q4) | P1-10 | — |
| CST shape decision (q2) | P3-01 | — |
| naga `glsl-in`: drop or keep (q1) | P5-02 | — |
| Compatibility-profile coverage (q3) | P4-07 | — |
| wasm size + reparse latency against §8 budgets | P1-09, P5-10 | — |
