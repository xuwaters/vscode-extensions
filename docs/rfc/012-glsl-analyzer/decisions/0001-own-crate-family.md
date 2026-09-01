# 0001 — New `crates/glsl/*` family, not an extension of `wgsl-syntax`

**Status:** Accepted (at RFC acceptance)

## Decision

The analyzer lives in four new crates — `glsl-spec`, `glsl-spec-gen`, `glsl-syntax`,
`glsl-analysis` — under `crates/glsl/`, mirroring the `crates/wgsl/` grouping-directory
pattern (explicit workspace members + directory exclude in the root `Cargo.toml`).
`wgsl-syntax` keeps its WGSL half untouched; its heuristic GLSL walk is retired once the
new pipeline proves parity (P3-08, P5-01), not before.

## Why

- `wgsl-syntax`'s contract is "resilient outline for both languages, no semantics". A
  preprocessor, a full grammar and a type checker are a different contract with a
  different shape; forcing them into the shared crate couples WGSL's stability to the
  largest rewrite in the extension.
- A dev-only generator binary (`glsl-spec-gen`) must not be a dependency of anything that
  compiles to wasm; a separate crate makes that structural rather than disciplinary.
- The repo's precedent (typst, fast) is one family per engine with `-core`/`-wasm`
  splits; `crates/glsl` follows it.

## Consequences

- `wgsl-lsp-core` grows a per-language branch at document level (it already has
  `Language`); features consume a common answer shape from either side.
- Two grouping directories must both be listed in root `Cargo.toml` members/excludes —
  scaffolded once, up front, so parallel phase work never edits shared manifests.
