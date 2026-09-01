# Reference material

## Ground rule

`temp/glslang` and `temp/glsl_analyzer` are **study material only. Never copy code,
comments, or data files from either into this repo.** Read them to learn what the
language *is* and how others carved it up; write our own implementation.

## temp/glslang — Khronos reference compiler (C++)

- **What to study**: `glslang/MachineIndependent/` — the preprocessor
  (`preprocessor/`), the grammar (`glslang.y` is the readable form of the official
  grammar), `Versions.cpp` (exact version/extension gating rules),
  `Initialize.cpp` (how the builtin surface varies by version/stage — sanity-check our
  generated spec against its *behaviour*, not its text).
- **Corpus**: `Test/` — ~2,000 shaders, every stage/version, including intentionally
  invalid ones. Used in place per [decision 0004](../decisions/0004-corpus-in-place.md).
- **Role**: behavioural oracle when the spec is ambiguous (preprocessor corner cases,
  implicit-conversion ranking). When we disagree with glslang, glslang wins unless the
  spec is explicit.
- License: mixed BSD-3/Apache-2.0/MIT per file — irrelevant while we copy nothing.

## temp/glsl_analyzer — Zig LSP

- **What to study**: `src/parse.zig`/`syntax.zig` (how small a recovering GLSL parser
  can be), `src/analysis.zig` (which semantic questions an LSP actually needs answered —
  a useful floor, though ours goes further into type checking), `spec/gen_spec.py` +
  `spec/spec.json` (what a docs.gl-derived spec contains; prior art for
  [decision 0002](../decisions/0002-docs-gl-as-spec-source.md)).
- **Useful negative lessons**: it skips real macro expansion and deep type checking —
  the two gaps this RFC exists to close.

## temp/docs.gl — the spec source (to be cloned by P1-01)

- github.com/BSVino/docs.gl → `temp/docs.gl`. Survey findings land in
  [docs-gl.md](docs-gl.md) (created by P1-01).

## The specifications themselves

- The OpenGL Shading Language 4.60 spec — grammar (§9), preprocessor (§3.3), types &
  conversions (§4.1), overload matching (§6.1), builtins (§8).
- GLSL ES 3.00/3.10/3.20 specs for the ES deltas; GLSL ES 1.00 for WebGL1-era files.
- These are the normative source for parser and semantics; glslang settles ambiguity.

## Existing in-repo prior art worth reading before writing code

- `crates/wgsl-shader/wgsl-syntax` — the house style for lexers/trees and the outline contract
  features currently rely on.
- `crates/wgsl-shader/wgsl-lsp-core/src/features/` — what the integration must feed.
- RFC 011 (`docs/rfc/011-fast-element-ultra/`) — the phase/task/decision conventions
  this RFC copies, and the corpus-gate pattern.
