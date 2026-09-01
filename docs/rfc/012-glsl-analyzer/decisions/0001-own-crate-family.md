# 0001 — Own `glsl-*` crates, not an extension of `wgsl-syntax`

**Status:** Accepted (at RFC acceptance). *Amended 2026-09-01:* originally the four
crates lived in their own `crates/glsl/` grouping directory; per user request they now
sit beside the wgsl crates in `crates/wgsl-shader/` (renamed from `crates/wgsl/`), so
all code for the extension is in one place. The crate *boundaries* below are unchanged.

## Decision

The analyzer lives in four new crates — `glsl-spec`, `glsl-spec-gen`, `glsl-syntax`,
`glsl-analysis` — beside the wgsl crates in the `crates/wgsl-shader/` grouping
directory (explicit workspace members + directory exclude in the root `Cargo.toml`).
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
  splits; `crates/wgsl-shader` follows it.

## Consequences

- `wgsl-lsp-core` grows a per-language branch at document level (it already has
  `Language`); features consume a common answer shape from either side.
- The grouping directory's members are listed by hand in the root `Cargo.toml` —
  scaffolded once, up front, so parallel phase work never edits shared manifests.
