# Phase 5 — LSP integration

**Goal:** route `Language::Glsl` documents in `wgsl-lsp-core` to the new pipeline; every
feature answers from it; settle naga's GLSL role (q1). Integration rules:
[design/architecture.md](../design/architecture.md#integration-into-wgsl-lsp-core).
**Needs:** Phase 4.

**Exit criterion:** all `wgsl-lsp-core` feature tests green for GLSL fixtures in ES,
desktop-OpenGL and Vulkan dialects; WGSL tests untouched and green; wasm size and
latency within §8 budgets.

**Crates touched:** `crates/wgsl-shader/wgsl-lsp-core` (and `wgsl-lsp-wasm` deps),
`crates/wgsl-shader/*` for API adjustments; `extensions/wgsl-shader` for settings surface.

| ID | Task | Test | Status | Notes |
| --- | --- | --- | --- | --- |
| P5-01 | The document pathway: `Document` holds the glsl tree+analysis for GLSL; an adapter feeds the existing feature shapes; the heuristic GLSL walk in `wgsl-syntax` is deleted **after** all other P5 feature tasks are green. | existing feature suite re-pointed | ☑ | `glsl/{mod,adapter}.rs`. `GlslDocument` holds raw tokens + `Preprocessed` + CST + `Outline` + `Analysis`; the adapter projects the outline onto `wgsl_syntax::Parsed` — kinds map name for name, blocks are paired over the raw stream, token kinds classified against `glsl-spec`. Walk deleted last: `parser/glsl.rs` (464) and `builtins/glsl.rs` (407) gone, GLSL arms of `builtins::*` return nothing. `index.rs` projects too (`GlslDocument::project`, outline only — indexing needs names, not types) |
| P5-02 | Diagnostics: ours become primary for GLSL; close q1 (drop vs. keep naga `glsl-in`) as [decisions/0008](../decisions/0008-naga-glsl-in-dropped.md) with Vulkan-fixture parity evidence; delete `analysis/dialect.rs` skip machinery accordingly. | diagnostics tests across all three dialects | ☑ | Preprocessor + parser + semantic diagnostics published with their `GLSL####` codes and `source: "glsl"`. q1 **closed: drop** — 17 Vulkan fixtures, zero naga-only findings, one we catch and it does not; `dialect.rs` (332) and `glsl.validate.dialect` deleted, naga is `features = ["wgsl-in"]` |
| P5-03 | Hover: builtin functions/variables/keywords from `glsl-spec` docs (availability-aware), typed hovers for user symbols (declared type + resolved detail). | hover tests | ☑ | `features/hover/glsl.rs`, driven by the resolved `Target` rather than a name lookup. Overloads filtered to the `#version`, `gl_*` variables name their stages, macros show their `#define`, swizzles and fields their type |
| P5-04 | Completion: context-aware — member/swizzle completion from types, builtins filtered by version/stage, qualifiers/layout keys in qualifier position, macros from the macro table. | completion tests per context | ☑ | `features/completion/glsl.rs`. Members from the analyzed base type (three swizzle sets, `length()` on arrays); builtins, legacy names, types and keywords all availability-filtered; stage filtering only when the stage was *declared*, never when guessed |
| P5-05 | Signature help: real overload sets with active-parameter tracking, generic families printed in spec notation. | signature tests | ☑ | `features/signature_help/glsl.rs`. Whole overload set, version-filtered, spec notation (`genType mix(genType x, …)`); active signature is the first whose arity still fits; per-parameter prose off the reference page. A file function shadows a builtin, and its own overloads are all shown |
| P5-06 | Definition/references/rename from resolved bindings (incl. shadowing, macro definition sites); rename refuses builtins gracefully. | def/refs/rename tests | ☑ | `features/definition/glsl.rs` reads the resolved `Target`, so shadowing and "the right struct's field" come free; macros resolve through the macro table. Rename refuses via `glsl-spec` + `glsl-analysis` instead of the retired table |
| P5-07 | Semantic tokens from CST + analysis (macro invocations, inactive regions dimmed via the standard modifier, resolved kinds instead of lexer guesses). | token snapshot tests | ☑ | `features/semantic_tokens/glsl.rs`. **Deviation:** LSP standardises no modifier for inactive code — its list stops at `defaultLibrary` — so the legend gains `disabled`, the conventional name clangd and VS Code's C/C++ legend use. Declaration sites fall back to the outline, which is also what paints an inactive branch |
| P5-08 | Symbols/folding/inlay hints migrated to the CST answers (folding gains inactive-region + block folds; inlay hints gain resolved types where the feature already shows them for WGSL). | per-feature tests | ☑ | Symbols come off the CST via P5-01's projection (macros and blocks appear now). Folding gains the inactive `#if` branch. Inlay hints: parameter names from the matched overload; the *type* hint has one thing to say in GLSL and it is the size an implicitly sized array takes from its initialiser — counted off the CST, since the type model records it as unsized |
| P5-09 | Settings & docs: `glsl.version`/dialect override surface re-thought now that validation is native (replacing `glsl.validate.dialect`). package.json contributions + README section. | settings round-trip test | ☑ | `glsl.validate.dialect` → **`glsl.defaultVersion`**: the question is no longer "should this be checked" but "which GLSL is this". Needed one addition to a completed crate — `glsl_analysis::Options::default_version`, used only for a file that declares no `#version`, with its own test in `glsl-analysis/tests/availability.rs`. `ShaderInfo` loses `skipped`, gains `stageGuessed`/`version`/`warnings`/`validator`; status bar shows the version and marks a guessed stage with `?`; README's naga-dialect section rewritten |
| P5-10 | Budgets measured: wasm size delta, 1k-line reparse+reanalyse latency native+wasm; recorded in [research/measurements.md](../research/measurements.md); §8 budgets asserted. | measurement script + recorded numbers | ☑ | **All four §8 budgets met and asserted.** Wasm **+46 KB** of ≤900 KB (gzip +6 KB) and **4.9 ms** of ≤25 ms (`src/wasmBudget.test.ts`, skips without `wasm/`); native **3.35 ms** of ≤5 ms and **3.48 ms** through the server (`tests/budgets.rs`, release only). The native figure was 6.9 ms when the features were finished; closing it took a sampling profiler and five structural fixes, none of which changed an answer — the linear scans of `glsl-spec`'s keyword and type tables became compile-time sorted indices with a first-byte reject; the lexer stopped resolving line continuations per character and matching punctuators against 47 spellings (1.21 → 0.16 ms); the CST builder stopped allocating a `Vec` per node; overload resolution stopped re-deriving every generic family member's type for every candidate (2.33 → 1.14 ms); the preprocessor stopped hashing every identifier against a macro table that almost never holds it. Also fixed earlier: `outline::references` was quadratic (2.56 → 0.44 ms). The recorded fallback order — memoise preprocessing, then cheapen analysis — was **not needed**: no feature was cheapened and no incremental state was added. The 597 pre-existing tests pass unmodified and the corpus gates are unchanged; two tests were added to guard the new compile-time orders. **Left on the table:** `PpToken` still owns a `String` per token, ~11 % of a rebuild — measurements.md §6 records why the arena that would remove it is not worth a three-crate API change against 1.6 ms of headroom |

## Notes on changed tests

Every WGSL test passes unmodified. The GLSL-side changes, each because the new
pipeline legitimately answers differently:

| Test | Why it changed |
| --- | --- |
| `features::a_dialect_naga_cannot_parse_gets_no_squiggles_but_says_why` | ES is analysed now; rewritten as `a_glsl_es_shader_is_analysed_rather_than_skipped`, which also asserts a real error is reported |
| `features::an_opengl_style_shader_gets_no_squiggles_but_says_why` | same, for OpenGL GLSL. Its fixture also had to change: `gl_FragColor` at `#version 450` is a real error our analyzer catches and naga's skip hid |
| `features::an_unbalanced_brace_is_reported_by_the_syntax_layer_in_a_dialect_naga_skips` | the GLSL parser reports it, with `GLSL0105` and `source: "glsl"` |
| `analysis::tests::*` (5 GLSL cases) | naga has no GLSL front end to build the fixtures with; the WGSL half is unchanged and one test now exercises the GLSL *spelling* of a naga type directly |
| `wgsl-syntax` — 11 tests deleted | they tested the heuristic walk and the hand-written GLSL tables, both of which are gone. Equivalent coverage is `glsl-syntax`'s outline tests plus `glsl/adapter.rs`'s projection tests |

## What was deleted

| | Lines |
| --- | --- |
| `wgsl-syntax/src/parser/glsl.rs` — the heuristic token walk | 464 |
| `wgsl-syntax/src/builtins/glsl.rs` — the hand-curated builtin tables | 407 |
| `wgsl-lsp-core/src/analysis/dialect.rs` — the naga skip machinery | 332 |
| `wgsl-syntax/src/tests.rs` + `builtins/mod.rs` — the tests for the above | ~180 |
| naga's GLSL front end from `analysis/mod.rs`, `Analysis::{stage, stage_label, skipped}`, `Dialect`, `glsl.validate.dialect` | ~150 |
| **Total removed** | **~1,530** |
