# Phase 3 — Parser & CST

**Goal:** the lossless, recovering GLSL parser in `glsl-syntax`.
**Needs:** Phase 2.

**Exit criterion:** every corpus shader parses with zero panics; the CST round-trips the
source byte-for-byte; the CST-derived outline covers everything the old heuristic walk
found on the extension's `examples/`.

**Crates touched:** `crates/glsl/glsl-syntax`.

| ID | Task | Test | Status | Notes |
| --- | --- | --- | --- | --- |
| P3-01 | CST shape decision (q2): green/red vs. flat event/span arrays, node-kind inventory for the 4.60 grammar, trivia and inactive-region attachment. Write [design/cst.md](../design/cst.md) and [decisions/0006](../decisions/). | n/a — the doc + record | ☐ | |
| P3-02 | Declarations: full qualifier sequences, `layout(…)` contents, `precision`, struct declarations, interface blocks (named/anonymous instance, arrays of blocks), arrays (sized/unsized, C and GLSL array syntax), initialisers. | declaration fixtures | ☐ | |
| P3-03 | Functions: prototypes vs. definitions, parameter lists with qualifiers/arrays, `subroutine` tolerance. | function fixtures | ☐ | |
| P3-04 | Statements & expressions: full precedence table (spec §5.1), selection/iteration/jump, `switch`, comma, ternary, call vs. constructor vs. array-index disambiguation. | expression precedence snapshots | ☐ | |
| P3-05 | Recovery: sync at `;`/`}`, unclosed groups at EOF, half-typed member lines — the "keeps working while the source does not parse" contract. Property: parsing never panics on any prefix of a valid file. | prefix-fuzz test over fixtures | ☐ | |
| P3-06 | Dialect variances: ES (`precision` statements everywhere, `attribute/varying` at 100), legacy desktop, `#version`-independent tolerance (parse everything, let analysis judge). | dialect fixtures | ☐ | |
| P3-07 | Corpus gate: parse all of `Test/`; zero panics; byte-for-byte round-trip; error-count snapshot in Notes. | `corpus_parse` test | ☐ | |
| P3-08 | Outline extraction from the CST (symbols, scopes, references — the shapes `wgsl-lsp-core` consumes) with parity tests against the old walk on `examples/*.{vert,frag,comp}`. The old walk is **not** deleted here (that is P5-01). | parity tests | ☐ | |
