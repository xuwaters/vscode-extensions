# 0012 — Ship the bundled fonts in a companion extension

**Status**: Accepted
**Date**: 2026-08-18
**Amends**: [0004](0004-bundle-default-fonts.md)

## Context

Decision 0004 put typst's default font set in the VSIX as plain files, and that part still holds — the
fonts have to be on disk, and they must not be in the WASM heap. What it did not weigh is *release
cadence*.

The 0.3.0 package, by compressed size:

| Part | Size | Changes when |
| --- | --- | --- |
| `wasm/typst_lsp_wasm_bg.wasm` | 10.2 MB | the Rust crates or upstream typst change |
| `assets/fonts/` (17 files) | 6.4 MB | typst-assets changes — in practice, never |
| code, grammar, snippets, notices | 0.2 MB | most days |

Every fix to a TypeScript file reshipped 16.8 MB, 6.4 MB of which was byte-identical to the release before
it. The fonts are the one part with a cadence measured in upstream releases rather than in commits.

## Decision

**Move `assets/fonts/` into its own extension, `weixu.wx-vsce-typst-ultra-fonts`**, and read it from there
at server start. `extensions/typst-ultra-fonts/` contains no code: no `main`, no `contributes`, no
activation. It exists so the directory has an install location another extension can name.

Typst Ultra declares it in `extensionDependencies`, **and** tolerates its absence — with the caveat below,
which is the part worth knowing before touching either half:

- The declaration is what makes a gallery install pull the fonts in automatically, and what stops someone
  uninstalling them out from under a working editor.
- The tolerance treats missing fonts as a *fidelity* loss — the document typesets in system fonts and
  stops matching `typst compile` — which is worth a warning, not a dead server.

**The declaration wins over the tolerance.** VS Code will not activate an extension whose declared
dependency is not installed; it shows *"Cannot activate … because it depends on the … extension, which is
not installed"* and stops there. So for the case the fallback was written for — a sideloaded VSIX with no
companion — the fallback never runs. It covers only a companion that is installed but has no font files in
it, and it is kept because dropping `extensionDependencies` is a one-line change that makes all of it live.

Same rule in the development loop: the Extension Development Host enforces the dependency, so `F5` needs
the companion installed, or both packages passed as `--extensionDevelopmentPath`.

`src/lsp/bundledFonts.ts` resolves the directory, best candidate first:

| Source | Path | When |
| --- | --- | --- |
| `companion` | `getExtension(id).extensionPath` + `assets/fonts` | normal install |
| `in-place` | this extension's own `assets/fonts` | an install from before the split |
| `sibling` | `../typst-ultra-fonts/assets/fonts` | `F5` from a checkout, where nothing is installed |

A candidate counts only if it *contains font files*. `pnpm run clean` leaves an empty directory behind, and
indexing that would report success while producing a document set in whatever the host had lying around.

The WASM artifact stays in the main extension. It is the larger half, but it moves with the Rust code, so
splitting it would trade one 10.2 MB reship for two coupled releases and a version-skew failure mode —
glue and module out of step is a crash, where fonts and code out of step is nothing at all.

## Consequences

**Buys.** A code-only release is 10.4 MB instead of 16.8 MB. The font package is republished only when
typst-assets moves, which is roughly once per upstream typst release. The two versions are independent:
`extensionDependencies` names no version, and there is no protocol between them beyond "this directory
holds font files".

**Costs.** Two VSIXes to install rather than one, and a sideloading user who installs only the main
extension gets an inert extension, per the caveat above. The font licences — which are not uniform, and one of
which is GPL with a font exception — now have to be reproduced in the companion's `LICENSE.md`, so the
upstream notice is maintained in two places; `dump-fonts` still asserts the set has not changed underneath
either copy.

**Testing.** The server tests reach the fonts through `server/testFonts.ts`, one constant, because the path
now crosses a package boundary and should break in one obvious place rather than in six files.

## Revisit if

- The WASM artifact's cadence slows to the point where it, too, is mostly reshipped unchanged — then the
  same split applies to it, with a version check at load.
- Distribution moves entirely to a gallery, in which case the fallback chain in `bundledFonts.ts` is dead
  weight and `extensionDependencies` alone is enough.
- Option 3 from 0004 (download on demand into `globalStorage`) becomes attractive; this decision makes it
  *less* likely, since the reship cost that motivated it is now paid once.
