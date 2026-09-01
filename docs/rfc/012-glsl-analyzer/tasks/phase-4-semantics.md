# Phase 4 — Semantic analysis

**Goal:** `glsl-analysis` — scopes, the type model, conversions, overload resolution,
and the diagnostics catalogue.
**Needs:** Phase 3 (CST) and Phase 1 (spec tables).

**Exit criterion:** every catalogued diagnostic has a seeded-error fixture that produces
exactly it; the false-positive gate (curated valid corpus shaders → zero errors) is
green.

**Crates touched:** `crates/glsl/glsl-analysis`.

| ID | Task | Test | Status | Notes |
| --- | --- | --- | --- | --- |
| P4-01 | Scopes & resolution: global/function/block/`for`-header scopes, shadowing, redeclaration rules, interface-block member injection (anonymous blocks), struct type names, every identifier occurrence resolved or recorded unresolved. | resolution fixtures | ☐ | |
| P4-02 | The type model: scalars/vectors/matrices, arrays (incl. implicitly-sized), structs, opaque types; type equality and printable names. | unit tests | ☐ | |
| P4-03 | Implicit conversions (§4.1.10) and constructor legality (§5.4): scalar→vector splat, int→uint→float→double ranks, matrix constructors, struct/array constructors. | rule-by-rule tests | ☐ | |
| P4-04 | Member access: swizzles (xyzw/rgba/stpq, repetition, lvalue rules), struct fields, `length()` on arrays/vectors/matrices, indexing. | fixtures | ☐ | |
| P4-05 | Expression inference: all operators with component-wise semantics, matrix `*` special cases, comparisons, logical/bitwise gating by type, ternary unification. | inference snapshots | ☐ | |
| P4-06 | Call resolution: user functions + builtins, generic-family expansion from `glsl-spec`, best-match overload ranking (exact > promotion > conversion, §6.1), out/inout argument checking. | overload gauntlet incl. ambiguity cases | ☐ | |
| P4-07 | Version/stage/profile context (q3): builtin availability filtered by declared `#version`+profile and stage (from extension or heuristic), compatibility coverage decided and recorded as [decisions/0007](../decisions/). | availability tests (e.g. `texture` in 110 → error with hint) | ☐ | |
| P4-08 | Statement rules: return-type agreement, lvalue/const-ness, `discard` outside fragment, `break/continue` placement, unreachable-after-return tolerance (warning at most), `const` initialiser requirements. | seeded fixtures | ☐ | |
| P4-09 | The diagnostics catalogue: stable codes, severities, editor-grade messages; one seeded fixture per code. Doc: [design/diagnostics.md](../design/diagnostics.md). | fixture-per-code test | ☐ | |
| P4-10 | False-positive gate: curate an initial list (≥ 40 files) of valid corpus shaders across versions/stages; zero error-severity diagnostics on them. The list only grows. | `corpus_no_false_errors` test | ☐ | |
