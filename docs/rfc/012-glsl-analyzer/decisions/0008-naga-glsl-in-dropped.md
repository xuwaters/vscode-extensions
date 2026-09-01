# 0008 — naga's `glsl-in` feature is dropped; our analyzer is the only GLSL authority

| | |
| --- | --- |
| **Status** | Accepted |
| **Closes** | [q1](README.md#open-questions-each-closes-as-a-numbered-record) |
| **Owner** | Phase 5 (P5-02) |
| **Date** | 2026-09-01 |

## The question

After Phase 5 routes GLSL to `glsl-analysis`, does naga's GLSL front end stay as an
optional second opinion for Vulkan-dialect files, or go?

The RFC's leaning was **drop**, conditional on showing diagnostic parity on Vulkan
fixtures first — the one dialect naga actually implements, and therefore the only place
keeping it could buy anything.

## Decision

**Drop it.** `wgsl-lsp-core` now depends on naga with the `wgsl-in` feature only. naga
remains the WGSL authority and is untouched there (RFC 012 §2 G7, N5).

The `glsl.validate.dialect` setting and `analysis/dialect.rs`, which existed solely to
detect the sources naga would mangle and switch validation *off* for them, go with it.
Commit 64fc05c — "stop flagging OpenGL GLSL as broken" by not analysing it — is undone
in the direction it wanted: OpenGL GLSL is analysed now.

## The evidence

Seventeen Vulkan-dialect fixtures, one construct each, run through both analyzers at
`#version 450` with an explicit `set`/`binding` layout and separate texture/sampler
objects. Each row is the same file with one statement inserted into `main`.

| Case | naga | ours |
| --- | --- | --- |
| valid | — | — |
| unknown name | Unknown variable: nowhere | `GLSL0200` 'nowhere' is not declared here |
| call to a non-function | Unknown function 'v_uv' | `GLSL0201` 'v_uv' is a variable, not a function |
| unknown function | Unknown function 'nosuchfn' | `GLSL0201` 'nosuchfn' is not a function this shader declares |
| wrong argument count | Unknown function 'lambert' | `GLSL0212` 'lambert' takes 2 arguments, and this passes 1 |
| wrong argument type | Unknown function 'lambert' | `GLSL0213` 'normal' is a vec3 and this argument is a float |
| unknown member | Unknown field: nope | `GLSL0204` 'Camera' has no member called 'nope' |
| bad swizzle | Invalid swizzle for vector "w" | `GLSL0205` 'w' is component 4 and a vec2 has 2 |
| no matching overload | Unknown function 'dot' | `GLSL0210` no overload of 'dot' takes (float) |
| assignment to a uniform | Function [1] 'main' is invalid | `GLSL0215` this is a uniform and '=' would write to it |
| type mismatch | Function [1] 'main' is invalid | `GLSL0216` '+' has no meaning for a vec4 and a bool |
| condition is not a bool | Function [1] 'main' is invalid | `GLSL0218` a condition must be a bool, and this is a vec2 |
| constant index out of range | Can't resolve type: OutOfBoundsIndex | `GLSL0208` 7 is outside a mat4, which has 4 of them |
| break outside a loop | Function [1] 'main' is invalid | `GLSL0221` 'break' needs a loop or a switch to leave |
| return with a value in void | Function [1] 'main' is invalid | `GLSL0219` this function is void, so its 'return' takes no value |
| const without an initialiser | — | `GLSL0222` 'c' is const and has no value |
| array size not constant | Unexpected runtime-expression | `GLSL0225` an array size must be a constant expression |

Plus the three shipped examples (`examples/test.{vert,frag,comp}`), each in its own
stage: naga parses and validates all three, and our analyzer reports zero errors on all
three.

**naga-only findings: none.** Every error naga produced, we produce. One case runs the
other way — `const float c;` is an error the language states outright and naga does not
report, because a `const` with no initialiser never reaches its IR.

Two differences are worth naming, and both favour dropping:

- **Attribution.** naga's front end lowers to an IR before it validates, so five of the
  sixteen collapse to `Function [1] 'main' is invalid` with the span of the whole
  function. Ours name the construct and land on it.
- **Reach.** naga's answer is "unknown function" for *every* call-shaped mistake —
  wrong arity, wrong types, no such overload are one message. The distinction is what a
  user acts on.

## Why keeping it was rejected

A second opinion is only worth its cost if it can disagree usefully. It cannot here:

1. **It has nothing to add.** The parity table is the whole argument — zero naga-only
   findings across every diagnostic class the two share.
2. **It would have to disagree in public.** Two analyzers publishing to one Problems
   panel means either duplicate squiggles on every real error, or a merge rule nobody
   can predict from the outside.
3. **It costs wasm bytes** in a crate whose §8 budget is the tightest constraint in
   this RFC, for a code path that answers for one of three dialects.
4. **It reintroduces the skip.** Keeping naga means keeping the machinery that decides
   *when* to run it — which is `dialect.rs`, which is what RFC 012 exists to delete.

## Consequences

- `wgsl-lsp-core`'s naga dependency drops to `features = ["wgsl-in"]`.
- `analysis/dialect.rs` is deleted, and with it `Dialect`, `skip_reason` and the
  OpenGL-marker scan.
- `glsl.validate.dialect` is removed from the extension's contributed settings; P5-09
  covers the replacement surface and the migration note.
- `Analysis` (the naga wrapper) loses `stage`, `stage_label` and `skipped`: they were
  GLSL-only, and GLSL no longer goes through it. `wgsl/shaderInfo` answers for GLSL
  from `glsl-analysis`'s own context instead, and gains `stageGuessed` and `version`.
- The parity harness above was a one-off. What stays in the repo is the half that does
  not need naga: `wgsl-lsp-core/tests/glsl_dialects.rs` asserts that our analyzer
  catches each of these in **all three** dialects, not just Vulkan.

## Revisiting

The one thing that would reopen this is a class of error naga catches structurally and
we cannot — resource-binding conflicts across a whole pipeline, say. That is a
whole-module concern, not an editor one, and if it is ever wanted the place for it is a
`glsl-analysis` rule, not a second front end.
