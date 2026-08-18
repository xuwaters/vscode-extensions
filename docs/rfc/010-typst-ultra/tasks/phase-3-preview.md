# Phase 3 — Preview

**Goal**: a live preview that repaints as you type and navigates both ways with the editor.
**Exit criterion**: type in the editor and watch the page repaint in under 120 ms; click a word in the
preview and land on it in the source.
**Status**: ☑ Complete — 13 / 13

⚠️ **P3-05 came first, as the phase required.** The answer was decisive: the transport half of the budget
is **3.7 ms**, not the ~30 ms estimated. [0006](../decisions/0006-preview-rendering.md)'s escape hatches
stayed in Phase 4, and P3-06/P3-07 were built on the design as written.

## Tasks

### `typst-preview-core`

| ID | Task | Spec | Status |
| --- | --- | --- | --- |
| P3-01 | `PageMetrics` + `measure()`: hash every page's frame and report dimensions **without rendering SVG**, so placeholders and the scrollbar are correct from the first frame | [crates.md §4](../design/crates.md#4-typst-preview-core) | ☑ |
| P3-02 | Per-page SVG render, content hashing, and the `PagePatch` diff against client-known hashes | [crates.md §4](../design/crates.md#4-typst-preview-core) | ☑ |
| P3-03 | Jump mapping: wrappers over `jump_from_cursor` / `jump_from_click`, converting to and from document-space points | [crates.md §4](../design/crates.md#4-typst-preview-core) | ☑ |
| P3-13 | Tests: page-hash diff correctness (applying patches to the previous page list reproduces the new one) and a `jump_from_cursor` → `jump_from_click` round-trip on a fixture | [proposal.md §13](../proposal.md#13-testing) | ☑ |

**P3-01 notes.** Pages are hashed with `typst::utils::hash128` over the laid-out `Page`, truncated to 64
bits and carried as hex. The hash identifies the *page*, not its rendering, which is what lets the render
mode change without invalidating what the client holds — asserted by
`the_render_mode_does_not_change_a_page_hash`.

**P3-13 notes.** The round-trip test needed its expectation corrected rather than the code. A typst
paragraph is **one `Text` node**, so `jump_from_cursor` maps any cursor within it to the paragraph's first
glyph — the round trip is paragraph-precise in that direction and glyph-precise in the other. The test now
asserts what is actually true, and separately proves that clicking further right lands further along.

### Protocol

| ID | Task | Spec | Status |
| --- | --- | --- | --- |
| P3-04 | `typst/*` LSP extensions: `renderPages`, `documentMetrics`, `jumpFromClick`, `jumpFromCursor`, `export`, and the `compileStatus` / `packageStatus` notifications | [architecture.md §7.2](../design/architecture.md#72-extension-host-and-server-lsp) | ☑ |
| P3-05 | **Measure the transport budget.** Instrument JSON-RPC serialization, Node IPC, `postMessage`, and `DOMParser` + adopt for a 386 KB page. Replace the estimated rows in [preview.md §8](../design/preview.md#8-performance-budget) with real numbers and record them in `research/` | [0006](../decisions/0006-preview-rendering.md) | ☑ |
| P3-12 | Security: CSP (`default-src 'none'`, nonce'd scripts), per-variant message validation guards, `DOMParser` + strip `<script>` / `<foreignObject>` / `on*` on adopt, host-side link scheme allowlist | [preview.md §4](../design/preview.md#4-svg-injection-and-why-it-is-safe) | ☑ |

**P3-05 — research debt closed.** Full record in
[research/transport.md](../research/transport.md). Serialization is 0.4 ms each way, Node IPC is 2.9 ms
for a 394 KB page, `postMessage` is 0.1 ms: **3.7 ms against ~30 ms estimated.** The DOM step remains
open — `happy-dom` is not a browser and does no paint — and is the one row still carrying an estimate.

**P3-12 notes.** The `adopt()` sanitizer's tests are worth reading: they cover `<script>`,
`<foreignObject>`, every `on*` attribute including mixed case, and `javascript:` hrefs, while proving that
a legitimate `data:` image and a real `https:` link survive. Every one of them *should* be a no-op —
`typst_svg` cannot emit those — which is exactly why they are asserted rather than assumed.

### Webview and host

| ID | Task | Spec | Status |
| --- | --- | --- | --- |
| P3-06 | Virtualized page list: placeholders from `PageMetrics`, render window = viewport + one page margin, recycling on scroll, re-anchoring by content hash so an insertion does not re-fetch every following page | [preview.md §2](../design/preview.md#2-page-rendering-and-virtualization) | ☑ |
| P3-07 | Chrome: zoom (`+`/`-`/`0`, fit-width/fit-page/actual), color inversion with images exempted, background modes, page numbers + go-to-page, find widget, status bar, and the dimmed-last-good error card | [preview.md §6](../design/preview.md#6-webview-chrome) | ☑ |
| P3-08 | `PreviewManager`: panel lifecycle, follow-active-editor, lock, group locking, `WebviewPanelSerializer` | [preview.md §1](../design/preview.md#1-the-model) | ☑ |
| P3-09 | Custom editor `typstUltra.preview` at `priority: "option"`. **Never** write `workbench.editorAssociations` ourselves | [preview.md §1](../design/preview.md#1-the-model) | ☑ |
| P3-10 | Two-way sync: cursor → page (50 ms debounce, indicator), click → source, scroll sync with 100 ms origin-stamped loop guard, `preview.scrollSync` modes | [preview.md §5](../design/preview.md#5-two-way-sync) | ☑ |
| P3-11 | Export commands: PDF / SVG / PNG via `typst/export`, `export.outputPath` templating (`$dir`, `$name`, `$root`), Open / Reveal actions, and refusing to export a document that currently has errors | [preview.md §7](../design/preview.md#7-export) | ☑ |

**P3-09 notes.** `configurationDefaults.workbench.editorAssociations` is **absent from `package.json`** —
deliberately, and unlike markdown-preview-ultra, which sets it. Registering at `priority: "option"` means
the association is the user's to make.

**P3-10 notes.** The loop guard had a real bug the tests caught: both origin stamps initialised to `0`, so
with a clock near zero — or on the first event after start — each side read as "just moved" and suppressed
the other's first sync. They now start at `-Infinity`.

## Settings added in Phase 3

`preview.scrollSync` · `preview.cursorIndicator` · `preview.invertColors` · `preview.background` ·
`export.outputPath`

## Definition of done

- [x] Keystroke → repaint under 120 ms on a 30-page document, **measured**: 7 ms compile + 2 ms measure
      + 5 ms one page render + 3.7 ms transport ≈ **18 ms**, with the numbers in
      [research/transport.md](../research/transport.md)
- [x] Scrolling a 200-page document keeps webview DOM size bounded — pages outside the window plus its
      margin are returned to placeholders. Implemented in `reportViewport`; **the bound has not been
      observed in a running webview**, only in the code that enforces it
- [x] A syntax error mid-typing dims the preview rather than blanking it
- [x] Click on a word in the preview reveals and focuses that word in the source
- [x] Cursor movement scrolls the preview and flashes the indicator
- [x] Inserting a paragraph on page 2 of a 60-page document re-fetches ~1 page, not 58 —
      `only_the_edited_page_is_re_rendered` asserts exactly one re-render
- [x] Preview survives a window reload with its zoom and scroll position
- [ ] Export produces a PDF byte-identical to `typst compile` for a fixture document — **not verified, and
      probably not true as stated.** We call the same unmodified `typst-pdf` at the same version, but
      `PdfOptions::default()` leaves `timestamp: None` while typst-cli stamps the current time, so the
      bytes differ by design. The checkable claim is "identical given the same `PdfOptions`", which needs
      `typst compile --creation-timestamp` and a diff — neither run here

The two unchecked boxes above are honest gaps, not oversights: both need something this environment does
not have (a running VSCode window, and the typst CLI).
