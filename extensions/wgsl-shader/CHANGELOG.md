# Changelog

## 0.6.0

**GLSL is analysed by an analyzer written for this extension.** Every dialect,
not just the one an external front end happened to read.

Until now GLSL had two half-measures under it: a heuristic token walk that found
declarations by the "name followed by name" signal, and naga's GLSL front end
for validation — which implements *Vulkan* GLSL at `#version 440`/`450`/`460`
only. Everything else, which is most real GLSL, was highlighted and never
checked. 0.5.1 made that quiet rather than wrong by switching validation off for
the sources it could not read.

Both are gone, replaced by four new crates: a real preprocessor, the GLSL 4.60
grammar, a type model with overload resolution, and a builtin table generated
from the Khronos reference pages.

### Added

- **Every dialect is analysed**: GLSL 1.10–4.60 in the core and compatibility
  profiles, GLSL ES 1.00–3.20, across all six shader stages. A WebGL 2 shader,
  an OpenGL shader with combined `sampler2D` uniforms and driver-assigned
  bindings, and a Vulkan shader all get the same treatment.
- **A real preprocessor.** `#define` and `#undef` (object- and function-like),
  the `#if` family with constant-expression evaluation, `#version`,
  `#extension`, `#pragma` and `#line`. Macros are expanded for real, and every
  expanded token still knows the source bytes it came from — so a diagnostic
  inside a macro argument lands where you wrote it. Both sides of a conditional
  stay in the outline; only the live side is analysed, and the other is dimmed.
- **Semantic diagnostics with stable codes** — `GLSL0001`–`GLSL0025` from the
  preprocessor, `GLSL0100`–`GLSL0110` from the parser, `GLSL0200`–`GLSL0227`
  from semantic analysis. Unknown names, calls to non-functions, wrong arity,
  wrong argument types, no matching overload, bad swizzles, unknown members,
  writes to uniforms, non-bool conditions, `break` outside a loop, non-constant
  array sizes, and more.
- **A generated builtin spec**: 161 functions over 717 overloads and 31 `gl_*`
  variables, each with the reference page's own prose, per-parameter notes, and
  the desktop and ES versions and stages it exists in. Beside it, a hand-written
  table of the compatibility and GLSL ES 1.00 surface the reference pages do not
  document but shaders still use: 39 functions and 58 variables — `texture2D`,
  `ftransform`, `gl_FragColor`, `gl_ModelViewMatrix` — plus the `attribute` and
  `varying` keywords.
- **Version- and stage-aware answers.** Hover on a builtin shows every overload
  *your* `#version` has, and no others. Completion offers `gl_FragCoord` to a
  fragment shader and `gl_Position` to a vertex one, and neither the
  compatibility names nor the double-precision types to a shader that has no
  such thing. Signature help shows the whole overload set in the specification's
  generic notation, with the one whose arity still fits highlighted.
- **`glsl.defaultVersion`** — the `#version` to analyse a file that declares
  none as, written the way the directive is: `450`, `330 core`, `300 es`. A file
  with no `#version` is GLSL 1.10 by the specification, which is rarely what the
  author of a runtime-assembled fragment meant.

### Changed

- Diagnostics, hover, completion, signature help, definition, references,
  rename, semantic tokens, symbols, folding and inlay hints all answer for GLSL
  from the new pipeline. Semantic tokens gained macro invocations and the dimmed
  inactive branch; inlay hints gained the deduced size of an implicitly sized
  array.
- `wgsl/shaderInfo` reports the version it analysed a file as, and whether the
  stage had to be guessed.
- The analyzer is deliberately quiet where it is unsure: a file whose parse or
  preprocessing failed gets no semantic errors, a file with an `#extension` line
  gets no unknown-name errors, and a `gl_`-prefixed or vendor-suffixed name is
  never reported as unknown.

### Removed

- **`glsl.validate.dialect`.** It existed only to switch naga's Vulkan-only GLSL
  front end off for sources it could not read, and nothing skips validation any
  more. A leftover value in `settings.json` is ignored.
- naga's `glsl-in` feature. naga remains the WGSL authority and is untouched
  there.

### Under the hood

WGSL behaviour is byte-for-byte unchanged. The wasm binary grew 47 KB
uncompressed and 6 KB gzipped; a full reparse and reanalysis of a 1,000-line
shader takes 3.4 ms native and 4.9 ms through the wasm binding, against budgets
of 5 ms and 25 ms. The parser and analyzer are gauged against Khronos's own
1,677-shader test corpus with zero panics, and a curated list of 208 valid
shaders must produce zero errors.

## 0.5.1

- Stopped flagging OpenGL-style GLSL as broken: sources naga's front end could
  not read were highlighted and left unvalidated rather than reported as
  erroneous.

## 0.5.0

- Replaced the in-process providers with a real language server, running in a
  child process with the analysis compiled to WebAssembly.

## 0.4.0

- GLSL support: `.glsl`, `.vert`, `.frag`, `.comp` and the long-form
  extensions, with a grammar, completion, an outline, and validation through
  naga's GLSL front end. `/* glsl */`-tagged strings in Rust and JS/TS
  highlighted like their `/* wgsl */` counterparts. Shader stage resolved from
  `#pragma shader_stage(…)`, the file extension, then the built-ins in the
  source. naga upgraded to 30.0.1.

## 0.3.1 and earlier

WGSL only: the language server's ancestors, the TextMate grammars, the
`/* wgsl */` injections into Rust and TypeScript, and the rust-analyzer
highlighting hint. See the repository history.
