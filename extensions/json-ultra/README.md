# JSON Ultra

JSON, JSONC, JSON5, and JSON Lines tooling powered by a Rust parser
compiled to WebAssembly.

## Features

- **Syntax highlighting** — contributes a JSON5 grammar and language
  (`.json5`); JSON, JSONC and JSON Lines use VSCode's built-in grammars.
- **Diagnostics** for JSON5 and JSON Lines: syntax errors, flavor
  violations (comments/trailing commas/unquoted keys where the dialect
  forbids them), duplicate keys. JSON/JSONC stay with VSCode's built-in
  validator to avoid duplicate squiggles.
- **Formatting** for all four dialects. Comments are preserved and
  scalars are reproduced byte-for-byte; only shape is normalized. JSON
  Lines formats to one compact record per line.
- **Sort object keys recursively** — the `JSON Ultra: Sort Object Keys
  Recursively` command, or continuously during formatting via
  `jsonUltra.format.sortKeys`. Comments move with their keys. The order
  is case-insensitive with digit runs compared as numbers, so
  `[astro]` and `$schema` group ahead of the words and `item2` precedes
  `item10`; exact code points only break ties.
- **oxc integration** — when the workspace is configured for oxc's
  formatter (`.oxfmtrc.json`, `.oxfmtrc.jsonc`, `oxfmt.config.ts`,
  `oxfmt.config.mts`), formatting is delegated to the project's `oxfmt`
  binary (`--stdin-filepath`, so oxfmt's own config discovery applies).
  Key sorting still works: the buffer is sorted by the WASM analyzer
  before it reaches oxfmt. Control with `jsonUltra.format.oxc` and
  `jsonUltra.oxc.path`.
- **Outline, folding, and JSON-path hovers** for JSON5 and JSON Lines.
- **JSON Lines table preview** — a read-only virtualized table that
  expands top-level fields into columns. The text editor remains the
  default; open the table with the title-bar button, `JSON Ultra: Open
  JSON Lines Table`, or "Reopen Editor With…". Click a row number to
  reveal that line in the text editor; double-click a cell to copy it.

## Settings

| Setting | Default | Effect |
| --- | --- | --- |
| `jsonUltra.format.sortKeys` | `false` | Sort keys recursively on every format. |
| `jsonUltra.format.oxc` | `auto` | `auto` delegates to oxfmt when the project is configured for it; `never` always uses the built-in formatter. |
| `jsonUltra.oxc.path` | `""` | Explicit oxfmt binary; empty resolves `node_modules/.bin/oxfmt`, then PATH. |
| `jsonUltra.diagnostics.enabled` | `true` | Diagnostics for JSON5 / JSON Lines. |
| `jsonUltra.preview.maxRows` | `100000` | Row cap for the table preview. |
| `jsonUltra.preview.maxFileSizeBytes` | `33554432` | Preview refuses larger files. |

## Development

```sh
pnpm run build:wasm   # wasm-pack build of crates/json-analyzer → wasm/
pnpm run build        # tsdown: dist/extension.js + dist/webview.js
pnpm run test         # vitest (WASM suite self-skips without build:wasm)
pnpm run package      # .vsix
```

The parser lives in `crates/json-analyzer`: one tolerant JSON5-superset
parser serves every dialect; the flavor decides what gets *diagnosed*,
not what parses. Features are span-based and unit-tested with plain
`cargo test`; `src/wasm_api.rs` is the only file that knows WASM exists.
