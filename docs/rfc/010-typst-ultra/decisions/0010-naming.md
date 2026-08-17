# 0010 — `typst-ultra`, four crates under `crates/typst/`

**Status**: Accepted
**Date**: 2026-08-17
**Resolves**: OQ 7

## Context

Names are cheap to choose and expensive to change once they are in a marketplace listing, a settings
namespace, and a hundred `use` statements.

Repo convention for extensions is descriptive-noun plus an occasional `-ultra` suffix for the
"comprehensive rewrite" ones: `markdown-preview-ultra`, `vim-ultra`, alongside `log-viewer`,
`wgsl-shader`, `gitignore-generator`. Crates are `<domain>-<role>`: `markdown-engine`, `log-engine`,
`proto3-analyzer`, `vim-engine`.

The complication for crate names is that `typst-*` is a busy namespace. Checking crates.io:

| Candidate | crates.io |
| --- | --- |
| `typst-engine`, `typst-analyzer`, `typst-preview`, `typst-lsp`, `typst-world` | **taken** |
| `typst-session`, `typst-lsp-core`, `typst-preview-core`, `typst-lsp-wasm` | free |

`typst-lsp` is particularly bad to reuse: it is the name of the unmaintained predecessor to tinymist, so a
crate of that name in this repo would read as a fork of it.

## Decision

**Extension**: `typst-ultra`, published as `wx-vsce-typst-ultra`, display name "Typst Ultra", settings
namespace `typstUltra.`, command prefix `typstUltra.`.

**Crates**, all `publish = false`, under `crates/typst/`:

| Crate | Role |
| --- | --- |
| `typst-session` | `World`, VFS/font/package ports, compile session, export |
| `typst-lsp-core` | LSP dispatch and IDE features |
| `typst-preview-core` | Page SVG, hashing/diff, jump mapping |
| `typst-lsp-wasm` | `#[wasm_bindgen]` surface |

All four names are unclaimed on crates.io. Since the crates are `publish = false`, a collision would be
harmless to cargo — workspace members always win — but an unclaimed name avoids confusing a future reader
who greps for one.

Workspace registration needs the form verified in
[research/spike.md §9](../research/spike.md#9-workspace-layout-for-cratestypst), because `members = ["crates/*"]`
alone cannot express a grouping directory:

```toml
members = ["crates/*", "crates/typst/typst-session", …]
exclude = ["crates/typst"]
```

## Consequences

**Buys.** Consistent with repo convention; no crates.io collisions; the `typst-` prefix groups the four
crates in listings; `crates/typst/` groups them on disk.

**Costs.** New crates under `crates/typst/` must be added to `members` by hand — the `crates/*` glob does
not reach them. One line per crate, and [design/crates.md §7](../design/crates.md#7-build-and-workspace-registration)
says so.

**Alternatives.** `typst-studio` and `typst-lab` were considered for the extension. `-ultra` was kept for
consistency with the two existing comprehensive extensions in this repo.

## Revisit if

- The extension is ever published under a shared publisher where `typst-ultra` collides.
- The crates become genuinely reusable outside this repo and are worth publishing, at which point the
  names should be re-checked against crates.io before `publish` is flipped.
