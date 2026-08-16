# Log Viewer Ultra

A custom editor for `.log` files that renders ANSI escape sequences as real
colour, highlights lines with filter rules you define, and stays responsive on
multi-gigabyte files. Parsing, ANSI rendering, filtering and search run in a
Rust core compiled to WebAssembly; large files are indexed on a background
worker thread and only the visible window is rendered.

## Features

- **Opens `.log` files automatically.** The custom editor claims the
  filename patterns `*.log` and `*.log.*` at default priority, so plain logs
  and rotated ones (`app.log.1`, `service.log.2026-08-16`) open in the viewer
  rather than the text editor. **Open in Text Editor** — a button in the
  toolbar and in the editor title bar — switches back at any time, and **Log
  Viewer Ultra: Open in Log Viewer** goes the other way.

- **ANSI rendering.** SGR sequences become styled HTML: the 8 standard and 8
  bright foreground/background colours, 256-colour (`38;5;n` / `48;5;n`),
  24-bit truecolor (`38;2;r;g;b`), and bold, dim, italic, underline, inverse
  and strikethrough. The 16 named colours map to VS Code's
  `--vscode-terminal-ansi*` variables, so they follow your theme rather than
  being hard-coded. Style state carries across line boundaries the way a
  terminal handles it. Everything that is not SGR is quietly stripped —
  cursor moves and screen clears (`ESC [ 2K`, `ESC [ H`, `ESC [ ?25l`), OSC
  sequences including hyperlinks and window titles, DCS/SOS/PM/APC, charset
  designators, and bare carriage returns. `CRLF` is normalised to `LF`. Turn
  rendering off with the **ANSI** toolbar button to see the raw text with the
  escapes still stripped.

- **Filter rules colour whole lines.** A rule is a substring or a JavaScript
  regular expression, case-insensitive by default, with a CSS colour used as
  the line's background. Each line is tagged with the first rule that matches
  it. Out of the box — when you have configured neither `logViewer.filterSets`
  nor `logViewer.filters` — a built-in **Log Levels** set is used:

  | Rule | Pattern | Case | Colour |
  | --- | --- | --- | --- |
  | Error | `\b(ERROR\|FATAL\|CRITICAL\|SEVERE)\b` | sensitive | `#5a1f1f` |
  | Warn | `\bWARN(ING)?\b` | sensitive | `#5a4a1f` |
  | Info | `\bINFO\b` | sensitive | `#1f4a5a` |
  | Debug | `\bDEBUG\b` | sensitive | `#3a3a3a` |
  | Trace | `\bTRACE\b` | sensitive | `#2f1f5a` |

  So a log like this gets three coloured lines and one plain one:

  ```
  2026-08-16 09:12:44.108 INFO  [main] c.e.Bootstrap - starting, pid=4181
  2026-08-16 09:12:44.902 DEBUG [pool-1] c.e.db.Pool - opened 8 connections
  2026-08-16 09:12:45.331 WARN  [pool-1] c.e.db.Pool - slow acquire: 412ms
  2026-08-16 09:12:45.334 ERROR [http-7] c.e.api.Handler - upstream timeout
      at com.example.api.Handler.dispatch(Handler.java:118)
  ```

  Note that the highlight is per line and comes only from the rules — nothing
  else in the line is parsed. Timestamps, thread names, logger names, stack
  frames, JSON payloads, URLs and IP addresses are not recognised or coloured
  on their own; if you want them marked, write a rule for them.

- **Filter sets.** Rules are grouped into named sets you can switch on and off
  from the **Sets** dropdown — one set for a login flow, one for audio, one
  for whatever you are chasing today. Individual rules also appear as chips on
  the toolbar; click a chip to mute that rule without touching the set.
  Toggles made this way last for the editor session; the persisted defaults
  live in `logViewer.filterSets`.

- **A filter editor.** **Log Viewer Ultra: Edit Filter Sets** (also reachable
  from the Sets dropdown) opens a modal for creating sets and rules, with
  name, pattern, regex and case-sensitivity fields, a 16-swatch colour palette
  plus a colour picker for adding your own, and regex validation on save.
  Saving writes to workspace settings when a workspace is open, otherwise to
  user settings; custom palette colours go to `logViewer.colorPalette`. If no
  log viewer is open, the command opens the settings UI instead.

- **Two filter modes.** In *Highlight* mode every line is shown and matching
  lines get their rule's background. In *Only matching* mode the view collapses
  to just the lines that match an enabled rule. The **Highlight / Only
  matching** toolbar button switches between them.

- **Search.** The search box does substring or regex matching, with
  independent `.*` (regex) and `Aa` (case-sensitive) toggles, a match count,
  and `<mark>` highlighting of the matched substrings that survives the ANSI
  markup around them. Search composes with the filter mode: in *Only matching*
  mode you see the intersection of the search hits and the enabled rules.
  Escape clears the box.

- **Virtualised rendering.** Only the lines on screen (plus a small overscan)
  are in the DOM, so scrolling a large log stays smooth. Optional word wrap,
  an optional left gutter with the original file line number, and a font size
  independent of the editor's are all toggleable from the toolbar or by
  command. The `A+` / `A−` / `A0` buttons and the font-size commands step
  within 8–36 px.

- **Large files stream.** Above `logViewer.streamingThresholdBytes` (4 MB by
  default) the extension switches pipelines: a worker thread scans the file
  and records a byte offset every 1024 lines, and the viewer asks the host for
  200-line windows as you scroll, reading and rendering just those bytes. The
  first and last megabyte are pre-rendered at open, so the first page and the
  **End ⏭** button work before indexing has finished. A banner reports
  indexing progress; filter and search passes run incrementally over the whole
  file and stream their hits in as they are found (capped at 1,000,000 hits,
  after which the banner says so). `Ctrl+End` / `Cmd+End` jumps to the end of
  the file and `Ctrl+Home` / `Cmd+Home` to the start.

- **Change detection.** While a streamed file is open, the extension watches
  it and shows a *File changed on disk (+12.4 KB)* banner with a **Reload**
  button when its size or mtime moves. This is a prompt, not a live tail — the
  view does not follow the file until you reload.

- **The line index can be cached.** By default it is kept in memory only and
  rebuilt on the next open, which is fast because it is tiny (about 8 bytes
  per 1024 lines). `logViewer.indexLocation` can instead persist it to the
  extension's global storage, next to the log file, or to a directory you
  choose, so reopening a huge log skips the scan entirely. **Log Viewer Ultra:
  Clear Index Cache** empties the global-storage and custom directories;
  index files written next to logs are left alone.

## Settings

| Setting | Default | Description |
| --- | --- | --- |
| `logViewer.renderAnsi` | `true` | Render ANSI escape sequences as coloured text. When off, raw text is shown with escapes stripped. |
| `logViewer.wordWrap` | `false` | Wrap long lines. |
| `logViewer.lineNumbers` | `true` | Show the original file line number in a left gutter. |
| `logViewer.fontSize` | `0` | Font size in px, 8–48. `0` inherits the editor's font size. |
| `logViewer.filterMode` | `"highlight"` | `highlight` shows every line and colours the matches; `only-matching` shows only lines matching an enabled filter or the search text. |
| `logViewer.filters` | `[]` | Legacy flat list of filter rules. Used as a synthetic "Default" set when `filterSets` is empty. Prefer `filterSets`. |
| `logViewer.filterSets` | `[]` | Named groups of filter rules. Each set has `name`, optional `description`, optional `enabled`, and `filters` (each with `name`, `pattern`, optional `regex`, `caseSensitive`, `color`, `enabled`). |
| `logViewer.colorPalette` | `[]` | Extra CSS colours appended to the built-in palette in the filter editor. Colours added through the picker are stored here. |
| `logViewer.maxFileSizeBytes` | `8388608` | Largest file the non-streaming path will render in full; past this only the head is shown, with a banner. |
| `logViewer.streamingThresholdBytes` | `4194304` | Files on disk larger than this use the streaming pipeline (background index + windowed rendering). |
| `logViewer.indexLocation` | `"memory"` | Where the line index lives: `memory`, `globalStorage`, `adjacent` (a `.bin` beside the log, falling back to global storage if unwritable), or `directory`. |
| `logViewer.indexDirectory` | `""` | Directory for index files when `indexLocation` is `directory`. A leading `~/` is expanded. |
| `logViewer.indexAnchorStride` | `1024` | Record a byte anchor every Nth line. Larger shrinks the index and slightly slows goto-line. |
| `logViewer.indexCacheBudgetMB` | `100` | Size cap for the persistent index cache; least recently accessed files are evicted past it. Does not apply to `adjacent` files. |

The view settings (`renderAnsi`, `wordWrap`, `lineNumbers`, `fontSize`,
`filterMode`) are the starting state for each editor. The matching toolbar
buttons and commands change the open view only; edit the setting to change the
default.

## Commands

All commands are under the **Log Viewer Ultra** category.

- **Open in Log Viewer Ultra** (`logViewer.openInLogViewer`) — reopen the
  active text editor's document in the viewer.
- **Open in Text Editor** (`logViewer.openInTextEditor`) — reopen the active
  document in the default text editor. Also a button in the editor title bar.
- **Toggle ANSI Color Rendering** (`logViewer.toggleAnsi`)
- **Toggle Word Wrap** (`logViewer.toggleWrap`)
- **Toggle Line Numbers** (`logViewer.toggleLineNumbers`)
- **Toggle Filter Mode (Highlight / Only Matching)**
  (`logViewer.toggleFilterMode`)
- **Focus Search Box** (`logViewer.focusSearch`)
- **Increase Font Size** (`logViewer.fontSizeIncrease`)
- **Decrease Font Size** (`logViewer.fontSizeDecrease`)
- **Reset Font Size** (`logViewer.fontSizeReset`)
- **Edit Filter Sets** (`logViewer.editFilterSets`) — open the filter set
  editor, or the settings UI if no log viewer is active.
- **Clear Index Cache** (`logViewer.clearIndexCache`) — delete persisted index
  files from the global storage and custom index directories.

## Keyboard shortcuts

These are active while a Log Viewer Ultra editor has focus.

| Shortcut | Action |
| --- | --- |
| `Ctrl+F` / `Cmd+F` | Focus the search box |
| `Ctrl+=` / `Cmd+=` | Increase font size |
| `Ctrl+-` / `Cmd+-` | Decrease font size |
| `Ctrl+0` / `Cmd+0` | Reset font size |
| `Ctrl+End` / `Cmd+End` | Scroll to the end of the file |
| `Ctrl+Home` / `Cmd+Home` | Scroll to the start of the file |
| `Escape` | Clear the search box, or close the filter editor |
