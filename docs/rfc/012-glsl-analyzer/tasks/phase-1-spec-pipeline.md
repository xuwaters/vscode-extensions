# Phase 1 — The spec pipeline

**Goal:** `glsl-spec` (embedded builtin tables) + `glsl-spec-gen` (the generator), per
[design/spec-pipeline.md](../design/spec-pipeline.md) and
[decision 0002](../decisions/0002-docs-gl-as-spec-source.md).

**Exit criterion:** `mix`, `texture` and `gl_FragCoord` come back from the embedded
tables with correct overloads, docs and version masks (asserted in tests); regeneration
is deterministic; the §8 size budgets hold.

**Crates touched:** `crates/glsl/glsl-spec`, `crates/glsl/glsl-spec-gen` only.
(Skeletons and workspace wiring already exist — do not edit the root `Cargo.toml`.)

| ID | Task | Test | Status | Notes |
| --- | --- | --- | --- | --- |
| P1-01 | Clone docs.gl into `temp/docs.gl`; survey `sl4/` + `el3/` (page counts, prototype quirks, versions-table shape, ES version encoding, pages to exclude and why). Write [research/docs-gl.md](../research/docs-gl.md). | n/a — the doc is the deliverable | ☐ | |
| P1-02 | The spec data model in `glsl-spec` (hand-written half): function/overload/param/family/variable types, version & stage masks, doc-pool slices, sorted-name lookup. | unit tests on lookup + mask semantics | ☐ | |
| P1-03 | Generator: parse `sl4/` function pages → overloads with param flow, per-param docs, version masks. Loud per-page failure; exclusion list lives in the generator with reasons. | `glsl-spec-gen` unit tests on representative saved fixtures¹ | ☐ | |
| P1-04 | Generator: `gl_*` variable pages → typed, stage-associated variables (watch the `gl_Position` fieldsynopsis quirk). | ditto | ☐ | |
| P1-05 | Generator: `el3/` ES pages parsed and merged — one entry, two masks; ES-only builtins included. | merge unit tests | ☐ | |
| P1-06 | Hand-written tables: keywords (with version/profile of arrival), basic types, precision defaults per stage — from spec text, in `glsl-spec`. | spot tests | ☐ | |
| P1-07 | Description → markdown conversion with per-entry budget (strip boilerplate, entities, code spans; ellipsis for dropped tables). | conversion fixtures | ☐ | |
| P1-08 | Emit committed `generated/*.rs` with tool + docs.gl commit + attribution header; byte-identical on re-run. | determinism test (skips when `temp/docs.gl` absent) | ☐ | |
| P1-09 | Quality gates: signature spot-checks (`mix` overload count, `texture` gsampler families, `textureGather` ≥ 4.00 desktop, ES masks for `texture` in 300 es), totals within expected ranges, generated-source ≤ 1.5 MB. | `glsl-spec` gate tests | ☐ | |
| P1-10 | License research (q4): docs.gl + Khronos refpage terms; write the attribution wording; add to the extension's third-party notices. Close as [decisions/0005](../decisions/). | n/a — decision record + notice file | ☐ | |

¹ Fixture pages for generator tests: a handful of *small, structurally representative*
docs.gl pages may be needed as test fixtures. Before committing any page content, P1-10's
license answer applies — until it is settled, tests read from `temp/docs.gl` in place and
skip when absent (same pattern as the corpus,
[decision 0004](../decisions/0004-corpus-in-place.md)).
