# Phase 4 — Semantic analysis

**Goal:** `glsl-analysis` — scopes, the type model, conversions, overload resolution,
and the diagnostics catalogue.
**Needs:** Phase 3 (CST) and Phase 1 (spec tables).

**Exit criterion:** every catalogued diagnostic has a seeded-error fixture that produces
exactly it; the false-positive gate (curated valid corpus shaders → zero errors) is
green.

**Crates touched:** `crates/wgsl-shader/glsl-analysis`, plus the one hand-written table
[decision 0007](../decisions/0007-legacy-builtin-table.md) adds to
`crates/wgsl-shader/glsl-spec` (`src/legacy.rs`; the generated files are untouched).

| ID | Task | Test | Status | Notes |
| --- | --- | --- | --- | --- |
| P4-01 | Scopes & resolution: global/function/block/`for`-header scopes, shadowing, redeclaration rules, interface-block member injection (anonymous blocks), struct type names, every identifier occurrence resolved or recorded unresolved. | resolution fixtures | ☑ | `analyzer.rs` + `symbols.rs`. Two passes: every file-scope declaration first, then bodies — so a call above its definition resolves, which GLSL forbids and an editor needs. Declaration sites are recorded as references too |
| P4-02 | The type model: scalars/vectors/matrices, arrays (incl. implicitly-sized), structs, opaque types; type equality and printable names. | unit tests | ☑ | `types.rs`. `Unknown` is a first-class, *quiet* type — every rule returns early on it. All ~130 spec type names round-trip |
| P4-03 | Implicit conversions (§4.1.10) and constructor legality (§5.4): scalar→vector splat, int→uint→float→double ranks, matrix constructors, struct/array constructors. | rule-by-rule tests | ☑ | `conversions.rs`. Costs are ordinal and are what §6.1 ranking reads; `Constructed::Unsure` is the third answer that keeps unknown arguments quiet |
| P4-04 | Member access: swizzles (xyzw/rgba/stpq, repetition, lvalue rules), struct fields, `length()` on arrays/vectors/matrices, indexing. | fixtures | ☑ | `expr.rs`. A repeated swizzle is a value, not storage; `.length` is only ever the method |
| P4-05 | Expression inference: all operators with component-wise semantics, matrix `*` special cases, comparisons, logical/bitwise gating by type, ternary unification. | inference snapshots | ☑ | `expr.rs`. `matAxB * matCxA = matCxB` and the vector forms; a 2,000-term `+` chain is a depth test, not a stack overflow |
| P4-06 | Call resolution: user functions + builtins, generic-family expansion from `glsl-spec`, best-match overload ranking (exact > promotion > conversion, §6.1), out/inout argument checking. | overload gauntlet incl. ambiguity cases | ☑ | `calls.rs`. A family binds once; an unbound return family resolves by *shape* then *class*, which is how `gsampler2D` decides `gvec4`. An unknown argument never binds a family |
| P4-07 | Version/stage/profile context (q3): builtin availability filtered by declared `#version`+profile and stage (from extension or heuristic), compatibility coverage decided and recorded as [decisions/0007](../decisions/0007-legacy-builtin-table.md). | availability tests (e.g. `texture` in 110 → error with hint) | ☑ | `context.rs` + `builtins.rs` + `glsl-spec/src/legacy.rs`. 39 functions, 58 variables. Legacy window is 1.10–**1.50** (the corpus proves 1.30–1.50 accept it), ES 1.00 only. A guessed stage never reports; an `#extension` line switches gating off |
| P4-08 | Statement rules: return-type agreement, lvalue/const-ness, `discard` outside fragment, `break/continue` placement, unreachable-after-return tolerance (warning at most), `const` initialiser requirements. | seeded fixtures | ☑ | `body.rs`. `GLSL0226`/`GLSL0227` are warnings; missing-return is "no `return` anywhere", the weakest form that cannot fire on a file being typed |
| P4-09 | The diagnostics catalogue: stable codes, severities, editor-grade messages; one seeded fixture per code. Doc: [design/diagnostics.md](../design/diagnostics.md). | fixture-per-code test | ☑ | 28 codes, `GLSL0200`–`GLSL0227`. Each has a fixture producing *exactly* it and a corrected twin producing nothing |
| P4-10 | False-positive gate: curate an initial list (≥ 40 files) of valid corpus shaders across versions/stages; zero error-severity diagnostics on them. The list only grows. | `corpus_no_false_errors` test | ☑ | **208 files**, zero errors. Derived from glslang's own expectations (no `ERROR:` in `baseResults/`), minus six whose validity needs a command-line flag; exclusions tabulated in the test |

## Results

| Gate | Result |
| --- | --- |
| `cargo test -p glsl-analysis` | 116 tests + 1 doctest, green |
| `corpus_analyze` | 1,677 files, **0 panics**, 1,555 with no semantic error, 1,042 errors total (a corpus of deliberately broken shaders) |
| `corpus_no_false_errors` | **208 valid shaders, 0 errors** |
| `the_extensions_examples_analyse_cleanly` | the extension's three `examples/*.frag|vert` report nothing — RFC 012 §9.2's promise, ahead of Phase 5 |
| `cargo clippy -p glsl-analysis --all-targets` | clean |
| `cargo test -p glsl-spec` / `-p glsl-syntax` | 27 / 244, green (glsl-spec gained 3 tests for the new table) |

## What is deliberately conservative

Recorded here as well as in [design/diagnostics.md §1](../design/diagnostics.md), because
it is the phase's whole design and the next phase will be tempted to undo it:

- A file whose **parse or preprocessing** produced an error, or which `#include`s
  something we never followed, gets full resolution and types and **no error-severity
  diagnostic at all**.
- An **`#extension` line switches off** the unknown-name, availability and
  no-matching-overload rules. Extensions add builtins and this RFC models none (§2 N3).
- A **`gl_` prefix or a vendor suffix** (`EXT`, `NV`, `ARB`, …) means the name is never
  "unknown".
- A **stage** the host did not name never produces a diagnostic; a stage error needs a
  builtin that lives in exactly one stage, and that stage to be fragment or compute.
- A **profile the tables say nothing about** is not a profile a name is absent from —
  docs.gl's missing ES pages must not become "not in ES".

## Left for later

| Thing | Where it belongs |
| --- | --- |
| Precision qualifiers (`precision mediump float;` required in ES fragment shaders) | A later phase, if the false-positive risk can be contained |
| Layout qualifier validity, interface matching between stages | RFC 012 §10, deferred |
| Wiring the analysis into `wgsl-lsp-core` (hover, completion, signature help from `Analysis`) | P5-01 onward; `Analysis` already carries the types, the chosen targets and the struct table those features need |
