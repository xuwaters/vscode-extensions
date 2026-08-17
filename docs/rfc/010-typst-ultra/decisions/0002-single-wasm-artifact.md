# 0002 — Ship one WASM artifact, not per-platform binaries

**Status**: Accepted
**Date**: 2026-08-17

## Context

Every existing typst language server ships a native binary per `(os, arch)`. The VSIX either bundles one
target, ships as several platform-specific VSIXs, or downloads the binary at activation. All three produce
the same recurring failure modes: unsupported architectures, corporate proxies blocking the download,
macOS Gatekeeper quarantine, and a cross-compilation CI matrix to maintain.

Nine extensions in this repo already compile a Rust crate to WASM and load it from Node. The open question
was whether that scales to a typesetting compiler. The prototype says yes
([research/spike.md](../research/spike.md)).

## Decision

Compile the server to a single `wasm32-unknown-unknown` artifact via `wasm-pack --target nodejs`, ship it
in one VSIX, and run it on every platform.

Build profile: `opt-level = "s"`, `lto = true`, plus `wasm-opt -Os --strip-debug` at package time.

## Consequences

**Cost, measured** ([research/spike.md §3](../research/spike.md#3-artifact-size) and
[§4](../research/spike.md#4-compile-latency)):

| | Native | WASM |
| --- | --- | --- |
| Artifact | binary per (os, arch) | 22 MB `.wasm` / 8.4 MB gzipped |
| Cold compile, 75 pages | ~143 ms (30-page native baseline extrapolated) | 262 ms |
| Incremental recompile, 30 pages | ~13 ms | 6 ms |
| Threads | `rayon` parallel layout | single-threaded (`rayon` runs with 1 thread, no panic) |

The cold-compile tax is roughly 2×; the incremental path — the one that runs on every keystroke — is at
parity or better. That is the right trade for an editor.

**Buys.** No CI matrix, no code signing, no install-time download, no "unsupported platform" issues, and a
plausible future path to a browser build for vscode.dev since the artifact is already WASM.

**Costs.** A ~14 MB VSIX (8.4 MB wasm + ~5 MB fonts, see [0004](0004-bundle-default-fonts.md)), and
`wasm-opt` adds ~14 s to `package`.

## Revisit if

- Cold-compile latency on real-world documents (images, CeTZ diagrams, bibliographies — none of which the
  spike covered) turns out to be materially worse than the synthetic 2× tax.
- WASM memory becomes the binding constraint in a way [0005](0005-cache-eviction-policy.md) cannot fix.
- WASM threads (`wasm32-unknown-unknown` + atomics, or `wasm-bindgen-rayon`) become viable in the VSCode
  Node host, which would close most of the cold-compile gap and is worth re-measuring when it does.
