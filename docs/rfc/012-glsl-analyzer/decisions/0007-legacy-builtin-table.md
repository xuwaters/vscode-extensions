# 0007 — A hand-written table for the surface docs.gl does not document

**Status:** Accepted (P4-07). Closes open question **q3**.

## Decision

`glsl-spec` gains **one hand-written table**, `src/legacy.rs`, beside the existing
hand-written `keywords.rs`, and the generator stays docs.gl-only. It carries the
predeclared surface the reference pages document nowhere:

1. the **compatibility and ES 1.00** builtins — `gl_FragColor`, `texture2D`, the
   fixed-function matrices and varyings;
2. the **builtin constants** of 4.60 §7.3 — `gl_MaxDrawBuffers` and its ~28
   relatives, which are *current* in every version and simply have no page;
3. the **4.60 arrivals** docs.gl's tables stop short of — the atomic-counter
   operations and the group-vote functions.

Availability is expressed twice, because the compatibility profile is not a version:
each entry carries the two version masks the generated tables use *plus* a
`compatibility: bool` saying the name survives in `#version N compatibility` at any
version. `glsl-analysis` reads both through `Context::available`.

The entries reuse `BuiltinFunction`/`BuiltinVariable` verbatim, so overload
resolution has one code path rather than two. Their prose is **hand-written** —
`DocRef::EMPTY` plus a `doc: &'static str` — because the Khronos pages carry none for
these names and inventing Khronos text would be worse than writing our own
([decision 0005](0005-refpage-attribution.md) covers only the generated prose).

## Why the old leaning was not available

q3 asked "full legacy set or the pragmatic subset the corpus uses", expecting P1-01
to say what docs.gl documents. P1-01's answer was **none of it**
([research/docs-gl.md](../research/docs-gl.md) §8): not one page mentions
`gl_FragColor`, `gl_TexCoord[]`, `texture2D`, `attribute` or `varying`. "Whatever
docs.gl documents" would have left every WebGL1-era and every compatibility-profile
shader with an analyzer that cannot name half of what it reads — and those are a
large share of the GLSL in the world, and exactly the files RFC 012 §1.1 exists to
stop mis-analysing.

## Scope, from the corpus

Scoped by reading `temp/glslang/Test` — 1,677 shaders — and counting what is actually
used. The `gl_*` surface there is enormous (343 uses of `gl_ScopeSubgroup` alone), but
almost all of it is *extension* vocabulary, which RFC 012 §2 N3 deliberately does not
model. The table covers the **core legacy** names and the analysis stays silent about
everything else `gl_`-prefixed.

### Included — 39 functions

| Group | Names |
| --- | --- |
| Legacy texture lookups | `texture1D` `texture2D` `texture3D` `textureCube` and their `Proj`, `Lod` and `ProjLod` forms; `texture2DRect`, `texture2DRectProj` |
| Legacy depth comparison | `shadow1D` `shadow2D` and their `Proj`/`Lod`/`ProjLod` forms; `shadow2DRect`, `shadow2DRectProj` |
| Fixed function | `ftransform` |
| 4.60, undocumented | `anyInvocation` `allInvocations` `allInvocationsEqual`; `atomicCounterAdd` `atomicCounterSubtract` `atomicCounterMin` `atomicCounterMax` `atomicCounterAnd` `atomicCounterOr` `atomicCounterXor` `atomicCounterExchange` `atomicCounterCompSwap` |

Corpus counts behind the texture set: `texture2D` 32 call sites, `texture2DProj` 11,
`texture3D` 8, `shadow2D`/`shadow2DProj`/`texture1D`/`textureCube`/`texture2DRect` and
the rest between 1 and 4 each. The `Lod`, `Proj` and `Rect` variants are included even
where the corpus uses one of them once, because a table with `texture2D` and without
`texture2DProj` would be a table that reports a false error the first time a real
shader projects a coordinate.

### Included — 58 variables

| Group | Names |
| --- | --- |
| Fragment outputs | `gl_FragColor` `gl_FragData` |
| Varyings and attributes | `gl_Color` `gl_SecondaryColor` `gl_FrontColor` `gl_BackColor` `gl_FrontSecondaryColor` `gl_BackSecondaryColor` `gl_TexCoord` `gl_FogCoord` `gl_FogFragCoord` `gl_Normal` `gl_Vertex` `gl_MultiTexCoord0`–`3` `gl_ClipVertex` |
| Fixed-function matrices | `gl_ModelViewMatrix` `gl_ProjectionMatrix` `gl_ModelViewProjectionMatrix` `gl_NormalMatrix` `gl_TextureMatrix` and the `Inverse`/`Transpose` forms of the first three; `gl_NormalScale` |
| Lighting | `gl_LightSource` |
| Current, undocumented | `gl_DepthRange`; the 28 `gl_Max*`/`gl_Min*` constants of §7.3 |

### Excluded, deliberately

| Excluded | Why |
| --- | --- |
| `gl_LightSourceParameters`, `gl_DepthRangeParameters` and the other fixed-function *structs* | The variables that use them are in the table with those type names; the analysis types them `Unknown`, which resolves the name, answers a hover, and keeps every rule downstream silent. Modelling the structs would buy one member-name completion for `#version 120` shaders |
| `gl_FrontMaterial`, `gl_BackMaterial`, `gl_Fog`, `gl_LightModel`, `gl_TextureEnvColor` and the rest of the fixed-function state | Zero uses in the corpus. Unknown `gl_` names are never an error, so their absence costs a hover and nothing else |
| `gl_MultiTexCoord4`–`7` | Zero uses; `0`–`3` are there because they are what shaders actually bind |
| Every `GL_ARB_*`/`GL_EXT_*`/`GL_NV_*` builtin | RFC 012 §2 N3. A file that enables an extension gets no unknown-name and no availability diagnostics at all, which is the same protection by a different route |
| `noise1`–`noise4` | Deprecated *and* never implemented by any driver; one corpus use, in a file testing that it is rejected |

## The version windows, and why they are wider than "1.20"

The obvious mask for the legacy surface is "1.10 through 1.20, deprecated in 1.30".
The corpus says otherwise: `texture.frag`, `matrix.frag`, `forLoop.frag`,
`aggOps.frag`, `prepost.frag` and `simpleFunctionCall.frag` are all shaders glslang
accepts that use `gl_FragColor` or `texture2D` at `#version 130`, `140` and `150`.
GL 3.0 *deprecated* the surface and the core profile removed it, but GLSL kept
accepting it until the renumbering at 3.30.

So `D_LEGACY` is **1.10 through 1.50**, and `Context::compatibility()` treats every
desktop version ≤ 1.50 as compatibility — those versions have no profiles at all.
From 3.30 on, the names exist only under `#version N compatibility`, which is exactly
what `GLSL0223` reports and what `tests::availability` pins down in both directions.

ES is narrower and needs no such allowance: `texture2D`, `texture2DProj`,
`textureCube` and the `Lod` forms are **ES 1.00 only**, and are gone from `300 es`
onward. `texture1D`, `texture3D`, the rectangle forms and every `shadow*` never
existed in ES at all.

## Consequences

- WebGL1 and compatibility shaders get real answers: `gl_FragColor` has a type, a
  stage and a version; `texture2D(sampler2D, vec2)` resolves to an overload; a
  `#version 300 es` shader that uses either is told which spelling to use instead.
- The table is ~430 lines of hand-written Rust that a human has to keep true. The
  gates that keep it honest are `glsl-spec::tests` (well-formed, no overlap with the
  generated tables, availability matches the language) and the false-positive gate,
  which fails the moment a mask is too narrow.
- Regeneration stays a no-op on this file: `glsl-spec-gen` never writes outside
  `src/generated/`, and the sorted-table test proves the two sets do not overlap.
