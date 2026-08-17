# 0004 — Bundle typst's default fonts as VSIX assets

**Status**: Accepted
**Date**: 2026-08-17
**Resolves**: OQ 5

## Context

Typst's default document uses Libertinus Serif for text, New Computer Modern Math for equations, and DejaVu
Sans Mono for raw blocks. Without them, output silently differs from `typst compile` — the kind of bug
report that cannot be closed, only explained.

The font set is ~9.5 MB across 29 files. Three ways to get it to users:

1. Embed in the WASM via `typst-assets/fonts` — puts all 9.5 MB in the artifact *and* permanently in the
   WASM heap.
2. Ship as VSIX assets, load from disk on demand.
3. Ship nothing; rely on system fonts, with an optional "download typst fonts" command.

## Decision

**Ship the font files as plain VSIX assets** under `assets/fonts/`, loaded through the same synchronous
host callback the VFS uses. Not embedded in the WASM.

Fonts load in two stages:

1. **Index**: the host extracts `FontInfo` metadata (upstream `Serialize`/`Deserialize`, so it caches to
   disk keyed by `path + mtime + size`) and hands it to Rust to build the `FontBook`. No font bytes are
   retained.
2. **Data on demand**: `World::font(index)` calls back for the bytes of a face a document actually selects.

The same mechanism serves system-font discovery (`typstUltra.fonts.system`, default on) and
`typstUltra.fonts.paths`.

## Consequences

**Buys.** Byte-identical output to `typst compile` out of the box. The WASM artifact stays lean. A user
with 400 MB of installed fonts gets them all indexed while contributing ~2 KB of metadata each and **zero**
bytes to the WASM heap until a face is used.

**Costs.** ~5 MB of the ~14 MB VSIX. First-run system-font indexing is a real cost with no measurement
behind it yet — mitigated by loading bundled fonts first (correct for most documents), indexing system
fonts in the background, then recompiling once.

**Licensing — this needs an explicit decision before Phase 1 ships.** The bundled fonts are not under one
license, and one of them is GPL:

| Files | License |
| --- | --- |
| `LibertinusSerif-*.otf` | SIL OFL 1.1 |
| `NewCM*.otf` except `NewCM10-Regular.otf` | GUST Font License 1.0 |
| **`NewCM10-Regular.otf`** | **GPL-3.0-or-later + Font Exception + Distribution Exception** |
| `DejaVuSansMono*.ttf` | Bitstream Vera Fonts License |
| `Foxit*.pfb` | BSD-3-Clause (© 2014 PDFium Authors) |

The Font Exception means documents embedding the font do not become GPL, and the Distribution Exception
permits shipping it alongside other software — this is the basis on which typst itself distributes it.
`LICENSE.md` must reproduce every one of these texts, scoped to the files it covers. Details and the
fallback (drop `NewCM10-Regular.otf` and accept the fidelity loss) are in
[research/references.md §2](../research/references.md#2-fonts--and-the-one-that-needs-care).

## Revisit if

- VSIX size becomes a real complaint, in which case option 3 (download on demand, cached in
  `globalStorage`) is the fallback — the loading path is already lazy, so only the *source* of the bytes
  changes.
- Shipping a GPL-licensed font is judged unacceptable despite the exceptions.
- System-font indexing on a large font directory measures badly enough to need a different strategy.
