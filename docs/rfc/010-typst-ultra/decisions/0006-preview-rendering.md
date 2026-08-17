# 0006 — Per-page SVG preview; PNG as a low-memory mode

**Status**: Accepted
**Date**: 2026-08-17
**Resolves**: OQ 2

## Context

The preview needs page images that update as you type. Upstream offers two renderers:

- `typst_svg::svg(page, opts)` — vector, resolution-independent, real `<text>` runs (so the webview's find
  widget works), and diffable per page.
- `typst_render::render(page, opts)` — a tiny-skia raster pixmap at a given pixels-per-point.

Measured anatomy of one text-heavy A4 page
([research/spike.md §7](../research/spike.md#7-page-svg-anatomy)):

| Metric | Value |
| --- | --- |
| Page SVG | 386 KB (37 KB gzipped) |
| Glyph `<defs>` | 46 KB — only 12% |
| Body | 3,144 `<use>` elements at full float precision — the other 88% |
| Whole 30-page document | 11.4 MB |
| Render cost, one page | ~5 ms |

So SVG is high-fidelity but verbose, and the verbosity is in per-glyph positioning, not glyph outlines —
meaning there is no cheap win from sharing glyph definitions across pages.

## Decision

**SVG is the default and only Phase 3 rendering path**, made affordable by two mechanisms rather than by
making the payload smaller:

1. **Virtualized rendering.** Only pages in the viewport plus one page of margin are materialized;
   everything else is a correctly-sized placeholder. Webview DOM size is bounded by viewport size, not
   document length.
2. **Per-page content hashing.** The webview reports which page hashes it already holds; the server returns
   SVG only for pages whose hash changed. A typical keystroke ships one page.

**A PNG low-memory mode is an accepted optimization**, scheduled for Phase 4 rather than deferred
indefinitely. It is a genuine win for very large or graphics-heavy documents, where SVG page size grows
with content complexity while a raster page is bounded by its pixel dimensions.

- Setting: `typstUltra.preview.renderMode` — `svg` (default) | `png` | `auto`.
- `auto` switches to PNG when a page's SVG exceeds a threshold (~1 MB), per page, so a document with a few
  pathological diagram pages does not force the whole preview to raster.
- PNG pages lose zoom fidelity (re-render on zoom-step change) and text selection/find. Both are stated in
  the setting description; neither is acceptable as a default, which is why `svg` stays the default.

Export is a separate path with the opposite trade-off — completeness over latency — and uses
`typst_svg::svg_merged` for whole-document SVG. See
[design/preview.md §7](../design/preview.md#7-export).

## Consequences

**Buys.** Sharp text at any zoom, working find-in-preview, and cheap incremental updates — one page render
(~5 ms) and one ~386 KB string per keystroke.

**Costs.** The transport half of the latency budget is estimated, not measured
([design/preview.md §8](../design/preview.md#8-performance-budget)): ~50 ms of JSON-RPC + IPC + postMessage
+ DOM parse for a 386 KB page. Replacing those estimates with measurements is the first task of Phase 3.

**Escape hatches, in order**, if the transport estimate proves wrong:

1. Round `<use>` coordinates to 2 decimal places — targets the 88%, sub-pixel visual cost.
2. The PNG mode above, promoted from "Phase 4 optimization" to "Phase 3 necessity".

## Revisit if

- Phase 3's transport measurements exceed the ~65 ms budget, promoting PNG mode from optimization to
  requirement.
- Real-world CeTZ/Fletcher-heavy documents produce SVG pages far larger than the synthetic 386 KB, which
  the spike did not cover.
- Upstream gains a more compact vector output format (tinymist uses typst.ts's `reflexo-vec2svg` for
  exactly this reason; if an equivalent lands upstream, re-measure).
