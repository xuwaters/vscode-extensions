# Decision records — RFC 012

One numbered file per decision that shapes code. Records are written when the decision is
*made*, which is usually when a phase starts, not before. Never renumber.

## Index

| # | Decision | Status |
| --- | --- | --- |
| [0001](0001-own-crate-family.md) | Own `glsl-*` crates beside the wgsl ones in `crates/wgsl-shader/`, not an extension of `wgsl-syntax` | Accepted |
| [0002](0002-docs-gl-as-spec-source.md) | docs.gl is the builtin-spec source; generated tables are committed | Accepted |
| [0003](0003-preprocessor-provenance.md) | Full preprocessing with byte-span provenance; both conditional branches stay visible, only the live one is analysed | Accepted |
| [0004](0004-corpus-in-place.md) | glslang `Test/` corpus is read in place from `temp/`, tests skip when absent | Accepted |
| [0005](0005-refpage-attribution.md) | Khronos reference-page prose is OPL v1.0; attribution in the generated header, the extension's notices, and no page ever committed | Accepted |
| [0006](0006-flat-cst-arrays.md) | The CST is flat preorder arrays over the expanded token stream, not green/red trees | Accepted |
| [0007](0007-legacy-builtin-table.md) | One hand-written `glsl-spec` table for the compatibility, ES 1.00 and otherwise-undocumented surface; the generator stays docs.gl-only | Accepted |
| [0008](0008-naga-glsl-in-dropped.md) | naga's `glsl-in` feature is dropped; our analyzer is the only GLSL authority, and `dialect.rs` goes with it | Accepted |

All eight are Accepted and none was superseded. The RFC closed at Phase 6 with no
ninth record needed: nothing in the release phase changed a decision, and the
deferred work listed in [tasks/README.md](../tasks/README.md) belongs to whatever
RFC picks it up, with its own numbering.

## Open questions (each closes as a numbered record)

| Q | Question | Owner | Leaning |
| --- | --- | --- | --- |
| ~~q1~~ | ~~Drop naga's `glsl-in` after Phase 5, or keep as optional Vulkan second opinion?~~ | Phase 5 (P5-02) | Closed by [0008](0008-naga-glsl-in-dropped.md): dropped. 17 Vulkan-dialect fixtures through both analyzers, zero naga-only findings; `analysis/dialect.rs` and `glsl.validate.dialect` deleted with it |
| ~~q2~~ | ~~CST shape: green/red trees vs. flat event/span arrays~~ | Phase 3 (P3-01) | Closed by [0006](0006-flat-cst-arrays.md): flat preorder arrays over the expanded stream, losslessness as a coverage function |
| ~~q3~~ | ~~Compatibility-profile builtin coverage: full legacy set or corpus-driven subset?~~ | Phase 4 (P4-07) | Closed by [0007](0007-legacy-builtin-table.md): a hand-written `glsl-spec` table beside the keywords tables — 39 functions and 58 variables, corpus-scoped, with a `compatibility` flag on top of the two version masks. Generation stays docs.gl-only |
| ~~q4~~ | ~~Exact docs.gl / Khronos refpage license text for the attribution notice~~ | Phase 1 (P1-10) | Closed by [0005](0005-refpage-attribution.md): OPL v1.0 for the Khronos prose, public domain for the docs.gl scaffolding |
