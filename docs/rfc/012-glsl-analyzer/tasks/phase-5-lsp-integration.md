# Phase 5 — LSP integration

**Goal:** route `Language::Glsl` documents in `wgsl-lsp-core` to the new pipeline; every
feature answers from it; settle naga's GLSL role (q1). Integration rules:
[design/architecture.md](../design/architecture.md#integration-into-wgsl-lsp-core).
**Needs:** Phase 4.

**Exit criterion:** all `wgsl-lsp-core` feature tests green for GLSL fixtures in ES,
desktop-OpenGL and Vulkan dialects; WGSL tests untouched and green; wasm size and
latency within §8 budgets.

**Crates touched:** `crates/wgsl/wgsl-lsp-core` (and `wgsl-lsp-wasm` deps),
`crates/glsl/*` for API adjustments; `extensions/wgsl-shader` for settings surface.

| ID | Task | Test | Status | Notes |
| --- | --- | --- | --- | --- |
| P5-01 | The document pathway: `Document` holds the glsl tree+analysis for GLSL; an adapter feeds the existing feature shapes; the heuristic GLSL walk in `wgsl-syntax` is deleted **after** all other P5 feature tasks are green. | existing feature suite re-pointed | ☐ | |
| P5-02 | Diagnostics: ours become primary for GLSL; close q1 (drop vs. keep naga `glsl-in`) as [decisions/0008](../decisions/) with Vulkan-fixture parity evidence; delete `analysis/dialect.rs` skip machinery accordingly. | diagnostics tests across all three dialects | ☐ | |
| P5-03 | Hover: builtin functions/variables/keywords from `glsl-spec` docs (availability-aware), typed hovers for user symbols (declared type + resolved detail). | hover tests | ☐ | |
| P5-04 | Completion: context-aware — member/swizzle completion from types, builtins filtered by version/stage, qualifiers/layout keys in qualifier position, macros from the macro table. | completion tests per context | ☐ | |
| P5-05 | Signature help: real overload sets with active-parameter tracking, generic families printed in spec notation. | signature tests | ☐ | |
| P5-06 | Definition/references/rename from resolved bindings (incl. shadowing, macro definition sites); rename refuses builtins gracefully. | def/refs/rename tests | ☐ | |
| P5-07 | Semantic tokens from CST + analysis (macro invocations, inactive regions dimmed via the standard modifier, resolved kinds instead of lexer guesses). | token snapshot tests | ☐ | |
| P5-08 | Symbols/folding/inlay hints migrated to the CST answers (folding gains inactive-region + block folds; inlay hints gain resolved types where the feature already shows them for WGSL). | per-feature tests | ☐ | |
| P5-09 | Settings & docs: `glsl.version`/dialect override surface re-thought now that validation is native (replacing `glsl.validate.dialect`); package.json contributions + README section. | settings round-trip test | ☐ | |
| P5-10 | Budgets measured: wasm size delta, 1k-line reparse+reanalyse latency native+wasm; recorded in [research/measurements.md](../research/measurements.md); §8 budgets asserted. | measurement script + recorded numbers | ☐ | |
