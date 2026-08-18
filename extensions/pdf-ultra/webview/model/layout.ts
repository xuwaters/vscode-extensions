import type { FitMode, PageMode, Rotation } from '../../src/messages.js';

/**
 * The continuous viewer's layout arithmetic, kept pure and away from the
 * element so it can be exercised without a rasterizer — happy-dom has no
 * canvas, and pdf.js needs one.
 *
 * The viewer models its own scroll geometry rather than measuring the DOM back:
 * every page has a box whether or not it currently holds a raster, so a pass
 * over 500 slots costs arithmetic instead of 500 forced reflows.
 */

/** One point is 1/72 inch; a CSS pixel is 1/96, so a point is 4/3 of a pixel. */
export const PX_PER_PT = 96 / 72;

/**
 * Padding around the page column, and the gap between pages. These must match
 * `styles.css`, which is the other half of the model.
 */
export const PAGE_PAD = 16;
export const PAGE_GAP = 16;

/** Bounds on the zoom, wherever it came from. */
export const MIN_ZOOM = 0.1;
export const MAX_ZOOM = 10;

/** The steps the zoom buttons walk through. */
export const ZOOM_STEPS: readonly number[] = [
  0.25, 0.33, 0.5, 0.67, 0.8, 1, 1.25, 1.5, 2, 3, 4, 6, 8, 10,
];

/** How many pages a mode puts side by side. */
export function columnsFor(mode: PageMode): number {
  return mode === 'dual' ? 2 : 1;
}

/** A page's unrotated size, in PDF points. */
export interface PageGeom {
  w: number;
  h: number;
}

/** A laid-out page box, in CSS pixels, in the scroller's coordinate space. */
export interface PageBox {
  w: number;
  h: number;
  top: number;
}

/** Turn a zoom (1 = actual size) into the scale pdf.js lays a viewport out at. */
export function viewportScale(zoom: number): number {
  return zoom * PX_PER_PT;
}

export function clampZoom(zoom: number): number {
  if (!Number.isFinite(zoom) || zoom <= 0) return 1;
  return Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, zoom));
}

/** A page's size as displayed, which swaps on a quarter turn. */
export function rotated(geom: PageGeom, rotation: Rotation): PageGeom {
  return rotation === 90 || rotation === 270
    ? { w: geom.h, h: geom.w }
    : { w: geom.w, h: geom.h };
}

/**
 * The zoom a fit resolves to for a scroller of this size. Fits measure against
 * the *content* box — the column's padding is part of the page's frame, not of
 * the page — and `actual` is the identity, which is what makes 100% mean 100%.
 *
 * `columns` is what a fit means in a two-page spread: the width is shared
 * between the pages of a row and the gap between them, so fitting the width
 * fits a *pair* of pages, not one page across the whole tab.
 */
export function fitZoom(
  fit: FitMode,
  view: { w: number; h: number },
  base: PageGeom,
  columns = 1,
): number {
  if (fit === 'actual') return 1;
  const perRow = Math.max(1, Math.round(columns));
  const availW = Math.max(
    1,
    (view.w - PAGE_PAD * 2 - PAGE_GAP * (perRow - 1)) / perRow,
  );
  const availH = Math.max(1, view.h - PAGE_PAD * 2);
  const widthZoom = availW / (base.w * PX_PER_PT);
  const heightZoom = availH / (base.h * PX_PER_PT);
  if (fit === 'fit-width') return clampZoom(widthZoom);
  // `fit-height` fills the height and lets a wide page overflow sideways;
  // `fit-page` takes whichever of the two is the smaller, so nothing overflows.
  if (fit === 'fit-height') return clampZoom(heightZoom);
  return clampZoom(Math.min(widthZoom, heightZoom));
}

/** The next zoom step in `direction`, or the end of the range. */
export function stepZoom(zoom: number, direction: 1 | -1): number {
  const steps = ZOOM_STEPS;
  const next =
    direction > 0
      ? steps.find((step) => step > zoom + 0.001)
      : [...steps].reverse().find((step) => step < zoom - 0.001);
  return clampZoom(next ?? (direction > 0 ? MAX_ZOOM : MIN_ZOOM));
}

/**
 * Stack the pages into rows of `columns`: box sizes at `scale`, and the top
 * edge of each in the scroller's coordinate space.
 *
 * The pages of a row share a top, and the row is as tall as the tallest of
 * them — so a short page next to a long one leaves the gap under it rather
 * than dragging the row below up into it. Each box keeps its *own* height,
 * which is what the visibility arithmetic wants: an empty strip beside a page
 * is not that page being on screen.
 */
export function stackPages(
  geoms: readonly PageGeom[],
  scale: number,
  rotation: Rotation = 0,
  columns = 1,
): PageBox[] {
  const perRow = Math.max(1, Math.round(columns));
  const boxes: PageBox[] = [];
  let top = PAGE_PAD;
  for (let start = 0; start < geoms.length; start += perRow) {
    let rowH = 1;
    for (let at = start; at < Math.min(start + perRow, geoms.length); at += 1) {
      const display = rotated(geoms[at]!, rotation);
      const h = Math.max(1, Math.round(display.h * scale));
      boxes.push({ w: Math.max(1, Math.round(display.w * scale)), h, top });
      rowH = Math.max(rowH, h);
    }
    top += rowH + PAGE_GAP;
  }
  return boxes;
}

/**
 * Stack one page and take the rest out of the flow — single-page mode.
 *
 * The pages that are not shown keep a slot in the array, so every index still
 * means the same page, but their boxes are empty: nothing measures them, the
 * scroll extent is the one page, and `currentPage` cannot pick one of them.
 * The column hides their elements to match, because a zero-height flex item
 * still collects the column's gap.
 */
export function stackSingle(
  geoms: readonly PageGeom[],
  index: number,
  scale: number,
  rotation: Rotation = 0,
): PageBox[] {
  return geoms.map((geom, at) => {
    if (at !== index) return { w: 0, h: 0, top: 0 };
    const display = rotated(geom, rotation);
    return {
      w: Math.max(1, Math.round(display.w * scale)),
      h: Math.max(1, Math.round(display.h * scale)),
      top: PAGE_PAD,
    };
  });
}

/**
 * Which page a point in the scroller falls on, and how far into it — the
 * anchor a zoom holds still, so the page under the pointer stays under it.
 *
 * Distinct from {@link currentPage}, which answers "what is the reader
 * looking at": this one is about a single y, and it never returns null,
 * because a wheel that lands past the last page still has to zoom something.
 */
export function anchorAt(
  boxes: readonly PageBox[],
  scrollTop: number,
  viewportY: number,
): { page: number; ratio: number } | null {
  if (boxes.length === 0) return null;
  const y = scrollTop + viewportY;
  const index = boxes.findIndex((box) => box.h > 0 && y < box.top + box.h);
  const at = index === -1 ? lastVisible(boxes) : index;
  const box = boxes[at];
  if (!box) return null;
  return { page: at + 1, ratio: box.h > 0 ? (y - box.top) / box.h : 0 };
}

function lastVisible(boxes: readonly PageBox[]): number {
  for (let index = boxes.length - 1; index >= 0; index -= 1) {
    if (boxes[index]!.h > 0) return index;
  }
  return 0;
}

/**
 * Should this page hold a raster at this scroll position? The visible band
 * grown by `screens` viewports either way — which is what makes scrolling down
 * continuous: the next page is drawn before it comes into view, and the pages
 * left behind release their canvases.
 */
export function isWithinBand(
  box: PageBox,
  scrollTop: number,
  viewH: number,
  screens = 1,
): boolean {
  const margin = Math.max(viewH * screens, screens > 0 ? 400 : 0);
  return (
    box.top + box.h >= scrollTop - margin && box.top <= scrollTop + viewH + margin
  );
}

/** How much of a page is actually on screen, in pixels. */
export function visibleHeight(box: PageBox, scrollTop: number, viewH: number): number {
  return Math.min(box.top + box.h, scrollTop + viewH) - Math.max(box.top, scrollTop);
}

/**
 * The page the reader is looking at, 1-based — the one showing the most of
 * itself, ties going to the earlier page. `null` when nothing is on screen (a
 * scroller that has not been laid out yet), which the caller reads as "leave
 * the counter alone".
 */
export function currentPage(
  boxes: readonly PageBox[],
  scrollTop: number,
  viewH: number,
): number | null {
  let best: number | null = null;
  let bestArea = 0;
  boxes.forEach((box, index) => {
    const visible = visibleHeight(box, scrollTop, viewH);
    if (visible > bestArea) {
      bestArea = visible;
      best = index + 1;
    }
  });
  return best;
}

/** Where to scroll so a page sits at the top of the viewport. */
export function scrollTopFor(box: PageBox): number {
  return Math.max(0, box.top - PAGE_PAD);
}

/** How far into a page the viewport starts, as a fraction of the page's height. */
export function offsetRatio(box: PageBox, scrollTop: number): number {
  return box.h > 0 ? (scrollTop - box.top) / box.h : 0;
}

/** The inverse: where to scroll to put the viewport `ratio` into a page. */
export function scrollTopAt(box: PageBox, ratio: number): number {
  return Math.max(0, box.top + ratio * box.h);
}

/**
 * Where a point in PDF user space lands on the displayed page, in CSS pixels
 * from its top-left corner.
 *
 * PDF space has its origin at the bottom-left and y pointing up; a display has
 * neither. Rotation is applied on top of that, as a clockwise turn of the
 * finished image — which is why the width and the height trade places on a
 * quarter turn.
 */
export function pointOnPage(
  base: PageGeom,
  rotation: Rotation,
  point: { x: number; y: number },
  scale: number,
): { x: number; y: number } {
  const { w, h } = base;
  const { x, y } = point;
  let dx: number;
  let dy: number;
  switch (rotation) {
    case 90:
      [dx, dy] = [y, x];
      break;
    case 180:
      [dx, dy] = [w - x, y];
      break;
    case 270:
      [dx, dy] = [h - y, w - x];
      break;
    default:
      [dx, dy] = [x, h - y];
  }
  return { x: dx * scale, y: dy * scale };
}

/**
 * The device-pixel ratio a page is rasterized at.
 *
 * Rendering at the display's own ratio is what makes text crisp, but a deep
 * zoom on an A0 poster would ask for a bitmap the GPU will not give us — and a
 * failed allocation is a blank page. Past the cap the raster goes coarse, which
 * is a page that looks soft instead of a page that is not there.
 */
export function rasterRatio(box: PageBox, dpr: number, maxPixels: number): number {
  const area = Math.max(1, box.w * box.h);
  const capped = Math.sqrt(maxPixels / area);
  return Math.max(0.1, Math.min(dpr, capped));
}
