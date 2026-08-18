import { describe, expect, it } from 'vitest';
import {
  MAX_ZOOM,
  MIN_ZOOM,
  PAGE_PAD,
  PX_PER_PT,
  clampZoom,
  fitZoom,
  pageSize,
  stepZoom,
  zoomBucket,
} from './layout.js';

/** A4, in points. */
const a4 = { widthPt: 595, heightPt: 842 };

describe('clamping a zoom', () => {
  it('keeps a sane one', () => {
    expect(clampZoom(1.25)).toBe(1.25);
  });

  it('holds the ends of the range', () => {
    expect(clampZoom(1000)).toBe(MAX_ZOOM);
    expect(clampZoom(0.001)).toBe(MIN_ZOOM);
  });

  // Nothing should ever hand this a NaN, but a page sized from one is a blank
  // tab rather than a visible failure, so it falls back to actual size.
  it('falls back to actual size for a value that is not a zoom', () => {
    expect(clampZoom(Number.NaN)).toBe(1);
    expect(clampZoom(0)).toBe(1);
    expect(clampZoom(-2)).toBe(1);
  });
});

describe('stepping the zoom', () => {
  it('goes up and comes back to where it started', () => {
    expect(stepZoom(stepZoom(1, 1), -1)).toBeCloseTo(1, 10);
  });

  it('stops at the ends rather than running past them', () => {
    expect(stepZoom(MAX_ZOOM, 1)).toBe(MAX_ZOOM);
    expect(stepZoom(MIN_ZOOM, -1)).toBe(MIN_ZOOM);
  });
});

describe('resolving a fit', () => {
  it('fits the width to the content box, not the border box', () => {
    const view = { w: 595 * PX_PER_PT + PAGE_PAD * 2, h: 100 };
    expect(fitZoom('width', view, a4)).toBeCloseTo(1, 10);
  });

  it('lets a tall page overflow when fitting the width', () => {
    // A wide, short viewport: fitting the width asks for more height than there
    // is, and that is correct — the reader scrolls.
    const view = { w: 595 * PX_PER_PT + PAGE_PAD * 2, h: 200 };
    expect(fitZoom('width', view, a4)).toBeGreaterThan(fitZoom('page', view, a4));
  });

  it('fits the whole page inside the box', () => {
    const view = { w: 1200, h: 400 };
    const zoom = fitZoom('page', view, a4);
    const size = pageSize(a4, zoom);
    expect(size.w).toBeLessThanOrEqual(view.w - PAGE_PAD * 2 + 0.001);
    expect(size.h).toBeLessThanOrEqual(view.h - PAGE_PAD * 2 + 0.001);
  });

  it('means exactly 100% at actual size, whatever the box', () => {
    expect(fitZoom('actual', { w: 37, h: 11 }, a4)).toBe(1);
  });

  // A tab in the background is laid out at nothing. Resolving a fit against it
  // would land at the bottom of the range and that number would be written into
  // the layout — which is how a document comes back from a background tab at
  // 10%. The element refuses to ask; this only guarantees the answer is a zoom.
  it('never returns something outside the range for a collapsed box', () => {
    const zoom = fitZoom('width', { w: 0, h: 0 }, a4);
    expect(zoom).toBeGreaterThanOrEqual(MIN_ZOOM);
    expect(zoom).toBeLessThanOrEqual(MAX_ZOOM);
  });
});

describe('bucketing the zoom for raster pages', () => {
  it('holds a small nudge in the same bucket', () => {
    expect(zoomBucket(1)).toBe(zoomBucket(1.1));
  });

  it('changes bucket over a real step', () => {
    expect(zoomBucket(1)).not.toBe(zoomBucket(1.6));
  });
});
