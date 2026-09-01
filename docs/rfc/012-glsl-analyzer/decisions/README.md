# Decision records — RFC 012

One numbered file per decision that shapes code. Records are written when the decision is
*made*, which is usually when a phase starts, not before. Never renumber.

## Index

| # | Decision | Status |
| --- | --- | --- |
| [0001](0001-own-crate-family.md) | New `crates/glsl/*` family, not an extension of `wgsl-syntax` | Accepted |
| [0002](0002-docs-gl-as-spec-source.md) | docs.gl is the builtin-spec source; generated tables are committed | Accepted |
| [0003](0003-preprocessor-provenance.md) | Full preprocessing with byte-span provenance; both conditional branches stay visible, only the live one is analysed | Accepted |
| [0004](0004-corpus-in-place.md) | glslang `Test/` corpus is read in place from `temp/`, tests skip when absent | Accepted |

## Open questions (each closes as a numbered record)

| Q | Question | Owner | Leaning |
| --- | --- | --- | --- |
| q1 | Drop naga's `glsl-in` after Phase 5, or keep as optional Vulkan second opinion? | Phase 5 (P5-02) | Drop, once diagnostic parity on Vulkan fixtures is shown |
| q2 | CST shape: green/red trees vs. flat event/span arrays | Phase 3 (P3-01) | Flat arrays, house style |
| q3 | Compatibility-profile builtin coverage: full legacy set or corpus-driven subset? | Phase 4 (P4-07) | Whatever docs.gl documents, and no more |
| q4 | Exact docs.gl / Khronos refpage license text for the attribution notice | Phase 1 (P1-10) | — |
