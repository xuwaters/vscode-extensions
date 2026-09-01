# 0003 — Full preprocessing with byte-span provenance

**Status:** Accepted (at RFC acceptance)

## Decision

`glsl-syntax` implements the GLSL preprocessor (spec §3.3) for real: object- and
function-like `#define`, `#undef`, the `#if` family with constant-expression evaluation
and `defined()`, plus capture of `#version`, `#extension`, `#pragma`, `#line`. Rules:

1. **Every token in the expanded stream carries a byte span into the original source.**
   A token that exists only because of expansion (it came from a macro *body*) maps to
   the span of the macro invocation that produced it; a token that came from a macro
   *argument* keeps the argument's own span.
2. **Inactive conditional regions stay in the tree** as skipped blocks — visible to the
   outline, folding, and syntax highlighting — but **only the live branch reaches the
   parser and semantic analysis**. Editing inside a dead branch must not produce
   spurious semantic errors.
3. Standard re-expansion rules apply (a macro is not re-expanded inside its own
   expansion); `__LINE__`, `__FILE__`, `__VERSION__`, `GL_ES` are predefined.

## Why

The heuristic walk's worst lies all trace to preprocessing: both branches of `#ifdef`
walked as live code, macros treated as plain names, versions unknown. Any semantic layer
built without expansion inherits those lies. Provenance is what keeps diagnostics,
hovers and renames landing on bytes the user can see; the invocation-site rule for
body-tokens is the only answer that never points into a `#define` the user isn't looking
at, and it matches what editors do for C.

## Consequences

- The preprocessor is the hardest single component and gets its own phase (2) with its
  own fixtures; glslang is the behavioural oracle when the spec under-specifies.
- `#include` is tolerated (parsed, not followed) per RFC §10.
