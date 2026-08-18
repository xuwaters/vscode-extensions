# PDF Ultra

A PDF viewer that is an editor, not an attachment. Open a `.pdf` and it renders
in the tab — continuous pages, real selectable text, find, an outline, and a
reload that keeps your place when the file is rebuilt beside you.

Built on [pdf.js](https://mozilla.github.io/pdf.js/) and entirely offline: no
CDN, no web fonts, no telemetry, no remote origin of any kind.

## What it does

**Reading**

- Continuous scrolling column, virtualized — a 500-page document costs about
  what a 5-page one costs, because only the pages within a screen of the
  viewport hold a raster
- A real text layer, so text selects, copies, and reads out to a screen reader
- Find in document, with every match on the page highlighted and `Enter` /
  `Shift+Enter` stepping through them
- Outline sidebar, collapsible and resizable, that follows where you are
- Link annotations work: internal ones jump within the document, external ones
  are handed to the host — the page itself never navigates
- Go back from a jump: follow a link, an outline entry, or a page you typed, and
  the toolbar's ← returns you to the exact spot you left, as far back as you went
- Zoom by step, by typed percentage (`150`, `150%`, `1.5x` all read the same),
  by `Alt`-wheel about the pointer, fit-width, fit-height, fit-page, or 100%
- One column or two pages side by side, both continuous — and one page at a
  time when you want the scrollbar to describe the page rather than the document
- Rotation, and colour inversion for reading white pages in a dark editor
- Page numbers, arrow keys, `PageUp`/`PageDown`/`Home`/`End`, and the page you
  are on in the status bar

**Living beside a build**

A PDF is usually an output. Change the file on disk — recompile the LaTeX, run
the Typst export, regenerate the report — and the tab reloads on the page you
were reading, at the zoom you were reading it at. Writes are debounced, so a
producer that rewrites the file in several passes does not flash an error at
you halfway through.

**Reopening**

Close a document and open it later and it comes back on the page you left it,
per workspace. Turn that off with `pdfUltra.rememberPosition`.

**Exporting** the current page as a PNG, at twice actual size, through a save
dialog. That is the extension's only write path, and it writes a new file — the
viewer is read-only by construction, so nothing it does can touch the document
it is showing.

## Getting started

Open a PDF. That is the whole setup — the extension registers itself as the
default editor for `*.pdf` and ships the matching `workbench.editorAssociations`
default, so nothing needs configuring and nothing else needs installing. Reopen
With gets you back to any other viewer you have; to make that choice stick,
point the association elsewhere:

```jsonc
"workbench.editorAssociations": { "*.pdf": "default" }
```

## Commands and keys

| Command | Key |
| --- | --- |
| Find in Document | `Cmd/Ctrl+F` |
| Go to Page… | `Cmd/Ctrl+G` |
| Go Back | `Cmd+[` / `Alt+←` |
| Zoom In / Out / 100% | `Cmd/Ctrl+=` / `Cmd/Ctrl+-` / `Cmd/Ctrl+0` |
| Fit Width / Fit Height | `Cmd/Ctrl+9` / `Cmd/Ctrl+8` |
| One Column / Two Columns, both continuous | `Cmd/Ctrl+1` / `Cmd/Ctrl+2` |
| Toggle Outline | `Cmd/Ctrl+K Cmd/Ctrl+O` |
| Toggle Colour Inversion | `Cmd/Ctrl+K Cmd/Ctrl+I` |
| Reload Document | `Cmd/Ctrl+K Cmd/Ctrl+R` |

In the page itself: `←` / `→` turn the page — unless the page is zoomed wider
than the tab, in which case they scroll it, because otherwise there would be no
way to reach its right-hand edge without a mouse. `Alt`-wheel zooms about the
pointer, so the spot you are looking at stays where it is.

Every shortcut above is an ordinary VS Code keybinding scoped to this editor, so
`Preferences: Open Keyboard Shortcuts` rebinds any of them — and if one of them
does something else on your setup, that is where the conflict will show.

Also in the palette, under **PDF Ultra**: next/previous page, fit page, rotate
either way, export the current page as a PNG, and open the file in whatever the
operating system uses for PDFs.

## Settings

| Setting | Default | What it does |
| --- | --- | --- |
| `pdfUltra.defaultZoom` | `fit-width` | Zoom a document opens at: `fit-width`, `fit-page`, `fit-height`, `actual` |
| `pdfUltra.background` | `editor` | Colour behind the pages |
| `pdfUltra.invertColors` | `never` | `always`, or `auto` to follow the theme |
| `pdfUltra.textLayer` | `true` | Selection, find, and screen-reader text |
| `pdfUltra.links` | `true` | Make link annotations clickable |
| `pdfUltra.outline.visible` | `false` | Show the outline by default |
| `pdfUltra.outline.width` | `240` | Sidebar width; drag its edge to change |
| `pdfUltra.reloadOnChange` | `true` | Reload when the file is rewritten |
| `pdfUltra.rememberPosition` | `true` | Reopen on the page last read |
| `pdfUltra.maxCanvasPixels` | `16777216` | Ceiling on one page's bitmap |
| `pdfUltra.renderAhead` | `1` | Screens rendered either side of the viewport |

## Privacy and safety

A PDF can come from anywhere, so the viewer treats every document as untrusted.
Nothing in the page may reach the network: no CDN, no web fonts, no telemetry,
no remote origin of any kind. Document scripting is not supported and the
JavaScript engine that would run it is not shipped. Links are rebuilt as the
viewer's own overlay and handed to VS Code, so clicking one never navigates the
page itself.

The extension never modifies the file it is showing. Exporting a page as a PNG
writes a new file, through a save dialog, and that is its only write.

Everything pdf.js needs to render — the cMaps for CID-keyed fonts (most CJK),
the 14 standard fonts, the JBIG2 / JPEG 2000 / ICC decoders — ships inside the
extension, so documents render the same offline as online.
