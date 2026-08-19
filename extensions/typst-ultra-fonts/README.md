# Typst Ultra Fonts

Typst's default font set, packaged as a VS Code extension so that **Typst
Ultra** (`weixu.wx-vsce-typst-ultra`) can ship without it.

There is no code here. The extension contributes nothing, activates never, and
exists only so that its `assets/fonts/` directory has a stable install location
that another extension can read.

## Why it is separate

The fonts are 6.4 MB of the VSIX, and they change when *typst-assets* changes —
which is roughly never, and never in step with the extension's code. Before the
split, every typo fix in a TypeScript file shipped those 6.4 MB again.

| | Before | After |
| --- | --- | --- |
| `wx-vsce-typst-ultra` | 16.8 MB | 10.4 MB |
| `wx-vsce-typst-ultra-fonts` | — | 6.4 MB |

The remaining 10.2 MB is the WASM engine, which does move with the code — see
`docs/rfc/010-typst-ultra/decisions/0012-fonts-in-a-companion-extension.md` in
the repository for why it stayed put.

## What is in it

17 files from [typst-assets](https://github.com/typst/typst-assets) 0.15.1,
written out by `pnpm run build:fonts`:

| Family | Files | Used for |
| --- | --- | --- |
| Libertinus Serif | 6 | body text |
| New Computer Modern Math | 3 | equations |
| New Computer Modern 10 | 4 | math-adjacent text |
| DejaVu Sans Mono | 4 | raw blocks |

They are build output, not source: `assets/fonts/` is gitignored, and
`dump-fonts` asserts the upstream set has not changed underneath the license
table in `LICENSE.md`.

## How Typst Ultra finds it

By extension id, at server start:

```ts
vscode.extensions.getExtension('weixu.wx-vsce-typst-ultra-fonts')
```

Typst Ultra declares this extension in `extensionDependencies`, so a gallery
install pulls it in automatically — and a **sideloaded** install of Typst Ultra
alone will not activate at all: VS Code blocks activation on a missing declared
dependency. Install this VSIX too. There is a fallback chain in
`extensions/typst-ultra/src/lsp/bundledFonts.ts` for the cases VS Code does let
through, and dropping the declaration is what makes it fully live.

The same rule applies to `F5`: the Extension Development Host wants this
extension installed, or both packages passed as `--extensionDevelopmentPath`.

`extensionKind` is `["workspace", "ui"]` because the compile happens where the
files are: in a remote window, the fonts have to land on the remote side.

## Releasing

Only when typst-assets changes. Bump the version, rebuild, package:

```sh
pnpm --filter wx-vsce-typst-ultra-fonts run package
```

Typst Ultra's `extensionDependencies` names no version, so the two are free to
move independently.

## License

`NO LICENSE` for the packaging. The fonts are third-party and their terms are
not uniform — one of them is GPL with a font exception. Every notice is in
`LICENSE.md`, which ships in the VSIX.
