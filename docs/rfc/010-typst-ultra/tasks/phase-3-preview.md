# Phase 3 — Preview

**Goal**: a live preview that repaints as you type and navigates both ways with the editor.
**Exit criterion**: type in the editor and watch the page repaint in under 120 ms; click a word in the
preview and land on it in the source.
**Status**: ☐ Not started — 0 / 13

⚠️ **P3-05 comes first.** Roughly 50 ms of the 65 ms preview latency budget is *estimated*, not measured
([preview.md §8](../design/preview.md#8-performance-budget)). If the estimate is wrong, the escape hatches in
[0006](../decisions/0006-preview-rendering.md) move from Phase 4 to Phase 3, which changes the shape of
P3-06 and P3-07. Measure before building on the assumption.

## Tasks

### `typst-preview-core`

| ID | Task | Spec | Status |
| --- | --- | --- | --- |
| P3-01 | `PageMetrics` + `measure()`: hash every page's frame and report dimensions **without rendering SVG**, so placeholders and the scrollbar are correct from the first frame | [crates.md §4](../design/crates.md#4-typst-preview-core) | ☐ |
| P3-02 | Per-page SVG render, content hashing, and the `PagePatch` diff against client-known hashes | [crates.md §4](../design/crates.md#4-typst-preview-core) | ☐ |
| P3-03 | Jump mapping: wrappers over `jump_from_cursor` / `jump_from_click`, converting to and from document-space points | [crates.md §4](../design/crates.md#4-typst-preview-core) | ☐ |
| P3-13 | Tests: page-hash diff correctness (applying patches to the previous page list reproduces the new one) and a `jump_from_cursor` → `jump_from_click` round-trip on a fixture | [proposal.md §13](../proposal.md#13-testing) | ☐ |

### Protocol

| ID | Task | Spec | Status |
| --- | --- | --- | --- |
| P3-04 | `typst/*` LSP extensions: `renderPages`, `documentMetrics`, `jumpFromClick`, `jumpFromCursor`, `export`, and the `compileStatus` / `packageStatus` notifications | [architecture.md §7.2](../design/architecture.md#72-extension-host-and-server-lsp) | ☐ |
| P3-05 | **Measure the transport budget.** Instrument JSON-RPC serialization, Node IPC, `postMessage`, and `DOMParser` + adopt for a 386 KB page. Replace the estimated rows in [preview.md §8](../design/preview.md#8-performance-budget) with real numbers and record them in `research/` | [0006](../decisions/0006-preview-rendering.md) | ☐ |
| P3-12 | Security: CSP (`default-src 'none'`, nonce'd scripts), per-variant message validation guards, `DOMParser` + strip `<script>` / `<foreignObject>` / `on*` on adopt, host-side link scheme allowlist | [preview.md §4](../design/preview.md#4-svg-injection-and-why-it-is-safe) | ☐ |

### Webview and host

| ID | Task | Spec | Status |
| --- | --- | --- | --- |
| P3-06 | Virtualized page list: placeholders from `PageMetrics`, render window = viewport + one page margin, recycling on scroll, re-anchoring by content hash so an insertion does not re-fetch every following page | [preview.md §2](../design/preview.md#2-page-rendering-and-virtualization) | ☐ |
| P3-07 | Chrome: zoom (`+`/`-`/`0`, fit-width/fit-page/actual), color inversion with images exempted, background modes, page numbers + go-to-page, find widget, status bar, and the dimmed-last-good error card | [preview.md §6](../design/preview.md#6-webview-chrome) | ☐ |
| P3-08 | `PreviewManager`: panel lifecycle, follow-active-editor, lock, group locking, `WebviewPanelSerializer` | [preview.md §1](../design/preview.md#1-the-model) | ☐ |
| P3-09 | Custom editor `typstUltra.preview` at `priority: "option"`. **Never** write `workbench.editorAssociations` ourselves | [preview.md §1](../design/preview.md#1-the-model) | ☐ |
| P3-10 | Two-way sync: cursor → page (50 ms debounce, indicator), click → source, scroll sync with 100 ms origin-stamped loop guard, `preview.scrollSync` modes | [preview.md §5](../design/preview.md#5-two-way-sync) | ☐ |
| P3-11 | Export commands: PDF / SVG / PNG via `typst/export`, `export.outputPath` templating (`$dir`, `$name`, `$root`), Open / Reveal actions, and refusing to export a document that currently has errors | [preview.md §7](../design/preview.md#7-export) | ☐ |

## Settings added in Phase 3

`preview.scrollSync` · `preview.cursorIndicator` · `preview.invertColors` · `preview.background` ·
`export.outputPath`

## Definition of done

- [ ] Keystroke → repaint under 120 ms on a 30-page document, **measured**, with the numbers written back into [preview.md §8](../design/preview.md#8-performance-budget)
- [ ] Scrolling a 200-page document keeps webview DOM size bounded
- [ ] A syntax error mid-typing dims the preview rather than blanking it
- [ ] Click on a word in the preview reveals and focuses that word in the source
- [ ] Cursor movement scrolls the preview and flashes the indicator
- [ ] Inserting a paragraph on page 2 of a 60-page document re-fetches ~1 page, not 58
- [ ] Preview survives a window reload with its zoom and scroll position
- [ ] Export produces a PDF byte-identical to `typst compile` for a fixture document
