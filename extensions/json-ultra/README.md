# JSON Ultra

The whole JSON family — JSON, JSONC, JSON5 and JSON Lines — over one Rust
parser compiled to WebAssembly.

The four dialects differ in what they *allow*, not in how they are read.
A `.json5` file and a `tsconfig.json` go through the same tolerant
parser here; the dialect only decides which constructs earn a squiggle.
So a stray comment in strict JSON still parses, still folds, still
formats — it just carries an error, and the rest of the file keeps
working while you fix it.

## What applies where

| | Files | Highlighting | Diagnostics | Formatting |
| --- | --- | --- | --- | --- |
| JSON | `.json` | VS Code's | VS Code's | JSON Ultra |
| JSONC | `.jsonc` | VS Code's | VS Code's | JSON Ultra |
| JSON5 | `.json5` | JSON Ultra | JSON Ultra | JSON Ultra |
| JSON Lines | `.jsonl`, `.ndjson` | — | JSON Ultra | JSON Ultra |

JSON and JSONC keep VS Code's own language service for validation and
IntelliSense — schema completion, `$schema` awareness, all of it — so
nothing is taken away and no line gets two squiggles for one mistake.
JSON5 and JSON Lines are where VS Code has nothing to say, and that is
where the outline, folding and hovers appear too.

## Diagnostics

For JSON5 and JSON Lines, each with a `JSON0xx` code and the same
flavor-aware reading:

- **Syntax errors** — an unexpected token, a missing `:` or `,`, a
  missing value, an unterminated string or block comment, content after
  the top-level value. Recovery is local, so one mistake does not turn
  the rest of the file into noise.
- **Flavor violations** — a comment, a trailing comma, a single-quoted
  string or an unquoted key where the dialect forbids it; hex literals,
  a leading `+`, `.5`, `5.`, `Infinity` and `NaN` outside JSON5. Each is
  reported against the dialect the file actually is, so JSON Lines is
  held to strict JSON per line while a `.json5` file is not.
- **Duplicate keys** in one object, and raw control characters inside a
  string.

They are debounced as you type and cleared when the tab closes. Turn
them off with `jsonUltra.diagnostics.enabled`.

## Formatting

**Comments are preserved and scalars are reproduced byte-for-byte.**
Only *shape* is normalized: one member or element per line, indentation
from your editor's `tabSize` and `insertSpaces`, trailing commas
dropped, a single space after `:`. A number written `1e-7` stays
`1e-7`, a string keeps its own escapes, a JSON5 file keeps its single
quotes and its unquoted keys. Empty objects and arrays stay inline. The
document's own line endings and your `files.insertFinalNewline` are
honoured on the way out.

**A broken file is left alone.** If the parse hit a hole — an
unterminated string or comment, a missing value, content after the top
level — formatting reports no edits rather than rewriting a half-typed
document into something worse.

**JSON Lines formats to one compact record per line**, blank lines
dropped, spacing after `,` and `:` normalized. A `.jsonl` file that
somehow contains comments is refused instead: there is nowhere on a
one-record line to put them.

JSON and JSONC already have VS Code's own formatter, so the first format
on one of those files asks which to use. Pick JSON Ultra there, or say
so once in your settings:

```jsonc
"[json]":  { "editor.defaultFormatter": "weixu.wx-vsce-json-ultra" },
"[jsonc]": { "editor.defaultFormatter": "weixu.wx-vsce-json-ultra" }
```

## Sort object keys recursively

Run **JSON Ultra: Sort Object Keys Recursively** from the palette, or
set `jsonUltra.format.sortKeys` and every format sorts as it goes. It
descends the whole document, and **comments move with their keys** — the
line documenting `port` is still above `port` afterwards, wherever
`port` landed.

The order is the one a reader expects rather than the one a byte
comparison gives: case-insensitive, with runs of digits compared as
numbers, and exact code points only breaking ties.

```jsonc
{
  "$schema": "…",   // punctuation sorts ahead of the words
  "[astro]": {},
  "Editor": {},     // next to "editor", not in a separate uppercase block
  "item2": {},      // before item10, because 2 < 10
  "item10": {}
}
```

## oxfmt delegation

When the workspace is configured for oxc's formatter — an `.oxfmtrc.json`,
`.oxfmtrc.jsonc`, `oxfmt.config.ts` or `oxfmt.config.mts` anywhere between
the file and the workspace root — formatting is piped through the
project's own `oxfmt` binary, with `--stdin-filepath`, so oxfmt's config
discovery and ignore files apply exactly as they do on the command line.
The binary is the one in the nearest `node_modules/.bin`, unless
`jsonUltra.oxc.path` names another.

Key sorting still works: the buffer is sorted by the WASM analyzer
*before* it reaches oxfmt, so both features compose. Anything short of a
clean run — no binary, a non-zero exit, a timeout — falls back to the
built-in formatter and says so in the output channel (**JSON Ultra: Show
Log**). Set `jsonUltra.format.oxc` to `never` to keep the built-in
formatter unconditionally.

## Outline, folding and hovers

For JSON5 and JSON Lines, where VS Code's own JSON service does not
reach:

- **Outline** down to the scalars, each node carrying its type and a
  short preview. A `.jsonl` file outlines as `Line 1`, `Line 2`, … with
  each record's fields beneath it, which is how you navigate one.
- **Folding** for every object, array and block comment in a JSON5 file,
  and `// #region` … `// #endregion` markers on top of that. JSON Lines
  records are one line each, so there is nothing to fold.
- **Hovers** naming the **JSON path** under the cursor —
  `$.servers[2].port` — with the value's type, its member or item count,
  and a preview of short scalars. In a `.jsonl` file the path is rooted
  at the record on that line.

## The JSON Lines table

`.jsonl` and `.ndjson` files can be read as a table: the union of the
records' top-level keys becomes the columns, in first-seen order, and
each line becomes a row. Records that are not objects — a bare array or
scalar on a line — land in a `(value)` column after the real keys, so a
mixed file still renders.

The text editor stays the default. Reach the table with the title-bar
button, **JSON Ultra: Open JSON Lines Table** (or *…to the Side*), or
"Reopen Editor With…".

- **Virtualized and read-only.** A few hundred recycled boxes cover any
  row count; a hundred-thousand-line file scrolls like a ten-line one.
  Nothing in the table writes to your file.
- **Click a row number** to reveal that line in the text editor beside
  the table.
- **Double-click a cell** to copy its value to the clipboard.
- The toolbar reports the record and column counts, and says so plainly
  when the row cap cut the file short.

To make the table the default for these extensions instead:

```jsonc
"workbench.editorAssociations": {
  "*.jsonl": "jsonUltra.jsonlPreview"
}
```

## Settings

| Setting | Default | |
| --- | --- | --- |
| `jsonUltra.format.sortKeys` | `false` | Sort keys recursively on every format |
| `jsonUltra.format.oxc` | `auto` | `auto` delegates to oxfmt when the project is configured for it; `never` always uses the built-in formatter |
| `jsonUltra.oxc.path` | `""` | Explicit oxfmt binary; empty resolves `node_modules/.bin/oxfmt`, then PATH |
| `jsonUltra.diagnostics.enabled` | `true` | Diagnostics for JSON5 and JSON Lines |
| `jsonUltra.preview.maxRows` | `100000` | Row cap for the table |
| `jsonUltra.preview.maxFileSizeBytes` | `33554432` | The table refuses larger files |

`format.sortKeys` is language-overridable, so you can sort `.json5` and
leave `.json` alone. The oxc and diagnostics settings are per-resource,
so a single workspace folder can differ from the rest.

## Privacy and safety

Nothing here reaches the network: no CDN, no web fonts, no telemetry, no
remote origin of any kind. The table's page loads one script from the
extension and gets its data over `postMessage`; its CSP is `default-src
'none'` with a per-load nonce.

The only file the extension writes is the one you are editing, through
VS Code's own edit and undo stack, so nothing is written until you save.
A cell holding `<script>` or a `javascript:` URL is shown as the text it
is, whoever sent you the file.

The one process ever spawned is `oxfmt`, and only when the project is
configured for it or `jsonUltra.oxc.path` points at one. Everything else
— parsing, diagnostics, formatting, the table — runs inside the
extension host.

