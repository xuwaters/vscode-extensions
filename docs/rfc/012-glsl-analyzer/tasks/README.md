# Tasks — RFC 012

**This folder is the source of truth for progress.** One file per phase; phases are
defined in [proposal.md §5](../proposal.md#5-phases). Whoever does the work updates the
phase file *in the same change*.

## Board

| Phase | Goal | Exit criterion | Status |
| --- | --- | --- | --- |
| [1 — Spec pipeline](phase-1-spec-pipeline.md) | `glsl-spec` + `glsl-spec-gen`, generated tables committed | `mix`/`texture`/`gl_FragCoord` correct from embedded tables; regeneration deterministic | ☑ 10 / 10 — 161 functions, 717 overloads, 31 variables; 47 tests |
| [2 — Lexer & preprocessor](phase-2-lexer-preprocessor.md) | Tokens, directives, macro expansion with provenance | Corpus lexes+preprocesses, zero panics; fixtures green | ☑ 7 / 7 — 1677 corpus files, 0 panics; 146 tests |
| [3 — Parser & CST](phase-3-parser.md) | Lossless CST, full grammar, recovery | Corpus parses, zero panics; outline parity on `examples/` | ☑ 8 / 8 — 1677 corpus files, 0 panics, byte-for-byte round-trip; outline parity on all three examples; 244 tests |
| [4 — Semantic analysis](phase-4-semantics.md) | Scopes, types, overloads, diagnostics | Every diagnostic has a seeded fixture; false-positive gate green | ☑ 10 / 10 — 28 codes `GLSL0200`–`GLSL0227`, one fixture each; 208-file false-positive gate at zero errors; 1677 corpus files, 0 panics; 116 tests |
| [5 — LSP integration](phase-5-lsp-integration.md) | GLSL routed to the new pipeline, all features | Feature tests green for ES/desktop/Vulkan; wasm budget held | ☑ 10 / 10 — every feature answers from the new pipeline in all three dialects; walk, tables and `dialect.rs` deleted (~1,530 lines); q1 closed by [0008](../decisions/0008-naga-glsl-in-dropped.md); 171 tests in `wgsl-lsp-core`, up from 129. **P5-10 ☑:** all four §8 budgets met and asserted — wasm +46 KB of 900 and 4.9 ms of 25 ms; native 3.4 ms of 5 ms, down from 6.9 by five structural fixes that changed no answer ([measurements](../research/measurements.md#5-closing-the-native-miss)) |
| [6 — Polish & release](phase-6-release.md) | Docs, notices, measurements, VSIX | Packaged VSIX; measurements recorded | ☑ 6 / 6 — `wx-vsce-wgsl-shader-0.6.0.vsix`, 833,062 B, packaged after a full green verification. `examples/` gained an ES 3.00 and a desktop-OpenGL shader beside the Vulkan three, all five under the zero-error gate; README documents GLSL per dialect and `glsl.defaultVersion`; CONTRIBUTING documents all seven crates, spec regeneration, corpus skipping and where each budget is asserted; notices are cargo-about-generated with the Khronos attribution in the template; [measurements §8](../research/measurements.md#8-the-release-sweep-p6-04) re-measures everything on the shipped tree — the wasm is byte-identical at 1,834,673 |

**RFC 012 is complete.** All six phases green, all four open questions closed by a
decision record, all five research debts discharged. What was deliberately left
undone is listed at the bottom of this file.

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
| docs.gl page inventory: uniformity, ES version encoding, per-page exclusions. **Gates the generator design.** | P1-01 | Closed — [research/docs-gl.md](../research/docs-gl.md). 314 uniform, well-formed pages; only `dFdy` stubs excluded; ES columns are 1.00/3.00/3.10 with 3.20 (and desktop 4.60) extrapolated |
| docs.gl / Khronos refpage license and required attribution (q4) | P1-10 | Closed — [decision 0005](../decisions/0005-refpage-attribution.md). Khronos prose is OPL v1.0, docs.gl scaffolding public domain; attribution generated into every table header plus `extensions/wgsl-shader/THIRD-PARTY-NOTICES.md` |
| CST shape decision (q2) | P3-01 | Closed — [decisions/0006](../decisions/0006-flat-cst-arrays.md), [design/cst.md](../design/cst.md). Flat preorder arrays over the expanded stream; the tree holds token indices, and losslessness is a coverage function rather than stored trivia |
| naga `glsl-in`: drop or keep (q1) | P5-02 | Closed — [decision 0008](../decisions/0008-naga-glsl-in-dropped.md): **dropped**. 17 Vulkan-dialect fixtures through both analyzers, zero naga-only findings, and one error (`const` with no initialiser) only ours reports. `dialect.rs` and `glsl.validate.dialect` went with it |
| Compatibility-profile coverage (q3) | P4-07 | Closed — [decision 0007](../decisions/0007-legacy-builtin-table.md). docs.gl documents **none** of the legacy surface ([research/docs-gl.md](../research/docs-gl.md) §8), so `glsl-spec` gained one hand-written table: 39 functions, 58 variables, corpus-scoped, with a `compatibility` flag beside the version masks. The legacy window is 1.10–1.50, not 1.10–1.20 — the corpus proves 1.30–1.50 still accept it |
| wasm size + reparse latency against §8 budgets | P1-09, P5-10 | Closed — [research/measurements.md](../research/measurements.md). **All four budgets met and asserted:** wasm **+46 KB** of ≤900 KB (gzip +6 KB) and **4.9 ms** of ≤25 ms; native **3.4 ms** of ≤5 ms, and **3.5 ms** through the server. The native figure started at 6.9 ms; §5 records the profile and the five structural fixes that closed it, none of which changed an answer. Two quadratic or near-quadratic costs were among them — `outline::references` (2.56 → 0.44 ms) and overload resolution re-deriving every family member's type per candidate (2.33 → 1.14 ms) |

## Deferred — the list for whoever writes the next RFC

Nothing here is a gap that was discovered late. Five items are
[proposal §10](../proposal.md#10-deliberately-deferred) restated, and the sixth
is the one performance item that was measured, priced and left. Each says what it
would cost, because a deferred item with no price is a wish.

| # | Deferred | Why it was left, and what it would take |
| --- | --- | --- |
| 1 | **`#include` (`GL_GOOGLE_include_directive`) and cross-file analysis** | The directive is parsed and tolerated, never followed. Following it means a resolver, an include graph, and invalidation across files — the first cross-document state in a pipeline that is deliberately per-document. It also needs a workspace notion of include paths that VS Code does not hand out for free. |
| 2 | **Per-extension (`GL_ARB_*`) builtin gating** | Availability is modelled as two version masks and a stage mask, which is what docs.gl encodes. An `#extension` line currently switches the *name* rules off wholesale rather than adding a specific surface. Doing it properly needs a source for which extension adds which builtin, and docs.gl is not one. |
| 3 | **A real GLSL formatter** | `glsl.format.enable` re-indents and changes nothing else. A pretty-printer wants the CST it already has, but it also wants a settled house style for a language with no `gofmt`, and every choice in it is an argument. |
| 4 | **Compute-shader-specific deep checks** | Local-size validation, shared-memory rules, barrier placement. None of it is in the `GLSL0200`–`GLSL0227` catalogue, and all of it is whole-module reasoning rather than the per-construct rules the catalogue is made of. |
| 5 | **Migrating WGSL off naga** | Out of scope forever as far as this RFC is concerned (§2 N5, G7). naga is the WGSL authority and stayed untouched throughout. |
| 6 | **`PpToken` owns a `String`** ([measurements §6](../research/measurements.md#6-what-is-still-there)) | The largest remaining allocation source: one `String` per code token, ~5,500 for the 1,000-line fixture, ~11 % of a rebuild, worth about **0.35 ms** against 1.6 ms of headroom. A `Cow` borrowing the source cannot work — `GlslDocument` owns both sides, so the borrow is self-referential. The right shape is a span into an arena the `Preprocessed` owns, which makes `PpToken::text` need the `Preprocessed` in hand and so changes the public API of three crates. Worth doing if the budget is ever tightened or a much larger file is seen to stutter; not worth it for a budget that is met. |
