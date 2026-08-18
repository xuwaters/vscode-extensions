import type { FitMode } from '../../src/preview/messages.js';

/**
 * The preview's zoom arithmetic, kept pure and away from both the DOM and the
 * element so it can be exercised without either.
 *
 * Everything here is about *how big a page is drawn*. Where the pages go is the
 * browser's business — the column is a flex stack, not a modelled scroll space,
 * because the compiler already told us every page's size and the list is short
 * enough that the layout engine can be trusted with it.
 */

/** One point is 1/72 inch; a CSS pixel is 1/96, so a point is 4/3 of a pixel. */
export const PX_PER_PT = 96 / 72;

/**
 * The padding around the page column and the gap between pages. These must
 * match `viewer/styles.css`: a fit measured against a padding the stylesheet
 * does not use overflows by exactly the difference.
 */
export const PAGE_PAD = 16;
export const PAGE_GAP = 16;

/** Bounds on the zoom, wherever it came from. */
export const MIN_ZOOM = 0.1;
export const MAX_ZOOM = 20;

/** What one press of the zoom buttons is worth. */
export const ZOOM_STEP = 1.2;

/** A page's size, as the compiler reports it. */
export interface PageGeom {
  widthPt: number;
  heightPt: number;
}

/** The size of the box a fit is measured against, in CSS pixels. */
export interface ViewSize {
  w: number;
  h: number;
}

export function clampZoom(zoom: number): number {
  if (!Number.isFinite(zoom) || zoom <= 0) return 1;
  return Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, zoom));
}

/** One step in `direction`, clamped to the range. */
export function stepZoom(zoom: number, direction: 1 | -1): number {
  return clampZoom(direction > 0 ? zoom * ZOOM_STEP : zoom / ZOOM_STEP);
}

/**
 * The zoom a fit resolves to for a scroller of this size.
 *
 * Measured against the *content* box — the column's padding is part of the
 * page's frame, not of the page — and `actual` is the identity, which is what
 * makes 100% mean 100%.
 *
 * A fit is resolved rather than remembered: the same fit is a different zoom
 * after the tab is resized, which is the whole point of it staying switched on.
 */
export function fitZoom(fit: FitMode, view: ViewSize, page: PageGeom): number {
  if (fit === 'actual') return 1;

  const availableWidth = Math.max(1, view.w - PAGE_PAD * 2);
  const availableHeight = Math.max(1, view.h - PAGE_PAD * 2);
  const widthZoom = availableWidth / (page.widthPt * PX_PER_PT);
  const heightZoom = availableHeight / (page.heightPt * PX_PER_PT);

  // `width` fills the width and lets a tall page run off the bottom, which is
  // what a reader scrolling through a document wants; `page` takes whichever of
  // the two is smaller, so a whole page is on screen at once.
  if (fit === 'width') return clampZoom(widthZoom);
  return clampZoom(Math.min(widthZoom, heightZoom));
}

/** A page's drawn size in CSS pixels. */
export function pageSize(page: PageGeom, zoom: number): ViewSize {
  return {
    w: page.widthPt * PX_PER_PT * zoom,
    h: page.heightPt * PX_PER_PT * zoom,
  };
}

/**
 * The zoom, bucketed to halves.
 *
 * A raster page is drawn at a fixed resolution, so a zoom step needs it drawn
 * again or it goes soft — but re-rendering on every nudge of a continuous zoom
 * would refetch the whole viewport for a change no one can see. Two steps
 * between 100% and 200% is the compromise.
 */
export function zoomBucket(zoom: number): number {
  return Math.round(zoom * 2);
}
