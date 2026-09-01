# Phase 1 — The spec pipeline

**Goal:** `glsl-spec` (embedded builtin tables) + `glsl-spec-gen` (the generator), per
[design/spec-pipeline.md](../design/spec-pipeline.md) and
[decision 0002](../decisions/0002-docs-gl-as-spec-source.md).

**Exit criterion:** `mix`, `texture` and `gl_FragCoord` come back from the embedded
tables with correct overloads, docs and version masks (asserted in tests); regeneration
is deterministic; the §8 size budgets hold.

**Crates touched:** `crates/wgsl-shader/glsl-spec`, `crates/wgsl-shader/glsl-spec-gen` only.
(Skeletons and workspace wiring already exist — do not edit the root `Cargo.toml`.)

| ID | Task | Test | Status | Notes |
| --- | --- | --- | --- | --- |
| P1-01 | Clone docs.gl into `temp/docs.gl`; survey `sl4/` + `el3/` (page counts, prototype quirks, versions-table shape, ES version encoding, pages to exclude and why). Write [research/docs-gl.md](../research/docs-gl.md). | n/a — the doc is the deliverable | ☑ | 314 real pages, all well-formed XML; three design amendments recorded in §9 (version rows ≠ prototype groups, 4.60/ES 3.20 columns absent, `gl_Position` quirk generalises) |
| P1-02 | The spec data model in `glsl-spec` (hand-written half): function/overload/param/family/variable types, version & stage masks, doc-pool slices, sorted-name lookup. | unit tests on lookup + mask semantics | ☑ | `version.rs` (two profile-typed mask families, 4.60/ES 3.20 extrapolated), `model.rs` (families as `FamilyId` data, prose as `DocRef` into one pool), binary-search lookup + `visible_in` |
| P1-03 | Generator: parse `sl4/` function pages → overloads with param flow, per-param docs, version masks. Loud per-page failure; exclusion list lives in the generator with reasons. | `glsl-spec-gen` unit tests on representative saved fixtures¹ | ☑ | Strict prototype reader: 1,168/1,168 parsed, `out`/`inout`, `[optional]`, sole-`void`, multi-function pages; five type errata in `spec.rs` cited to §3.1 |
| P1-04 | Generator: `gl_*` variable pages → typed, stage-associated variables (watch the `gl_Position` fieldsynopsis quirk). | ditto | ☑ | 31 variables; the `gl_PerVertex` listing pages (`gl_Position`, `gl_PointSize`) read from the `programlisting`, twin `fieldsynopsis` pages merged to one entry with `InOut`, array suffix folded into the type |
| P1-05 | Generator: `el3/` ES pages parsed and merged — one entry, two masks; ES-only builtins included. | merge unit tests | ☑ | One entry, two masks; 1,168 prototypes → 717 overloads; ES-only signatures (`texture(samplerCubeShadow, vec4)`) survive with an empty desktop mask |
| P1-06 | Hand-written tables: keywords (with version/profile of arrival), basic types, precision defaults per stage — from spec text, in `glsl-spec`. | spot tests | ☑ | `keywords.rs`: 43 keywords with arrival masks + a `compatibility` flag for `attribute`/`varying`, ~110 basic types, 38 reserved words, ES precision defaults per stage |
| P1-07 | Description → markdown conversion with per-entry budget (strip boilerplate, entities, code spans; ellipsis for dropped tables). | conversion fixtures | ☑ | `markdown.rs`: code spans, emphasis, lists, `glsl` fences, TeX and MathML handled; 1.2 KB per entry; 181 KB of page prose → a 78.7 KB pooled, deduplicated doc string |
| P1-08 | Emit committed `generated/*.rs` with tool + docs.gl commit + attribution header; byte-identical on re-run. | determinism test (skips when `temp/docs.gl` absent) | ☑ | 5 files, 424,200 bytes, header carries tool + docs.gl commit + Khronos/OPL attribution. `rm -rf generated && cargo run -p glsl-spec-gen` reproduces byte-identical output; `--check` mode + two in-tree determinism tests |
| P1-09 | Quality gates: signature spot-checks (`mix` overload count, `texture` gsampler families, `textureGather` ≥ 4.00 desktop, ES masks for `texture` in 300 es), totals within expected ranges, generated-source ≤ 1.5 MB. | `glsl-spec` gate tests | ☑ | 23 `glsl-spec` gate tests. Generated source 424,200 bytes against the 1.5 MB budget; static footprint 203 KB (78.7 KB prose) — wasm growth itself is P5-10, the crate is not linked in yet |
| P1-10 | License research (q4): docs.gl + Khronos refpage terms; write the attribution wording; add to the extension's third-party notices. Close as [decisions/0005](../decisions/). | n/a — decision record + notice file | ☑ | [decisions/0005](../decisions/0005-refpage-attribution.md): Khronos prose is OPL v1.0, docs.gl scaffolding public domain. Attribution generated into every table header; new `extensions/wgsl-shader/THIRD-PARTY-NOTICES.md`; no page committed, so the footnote below stands permanently |

¹ **Settled by P1-10, and the answer is permanent: no docs.gl page is committed, ever.**
Generator tests read `temp/docs.gl` in place and skip with a visible message when it is
absent — the same pattern as the corpus,
[decision 0004](../decisions/0004-corpus-in-place.md). The conversion tests that needed a
"structurally representative page" use hand-written fixtures instead, which turned out to
be better tests: they exercise one construct each.
See [decision 0005](../decisions/0005-refpage-attribution.md).
