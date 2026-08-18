import { describe, expect, it } from 'vitest';
import {
  MAX_ZOOM,
  MIN_ZOOM,
  PAGE_GAP,
  PAGE_PAD,
  PX_PER_PT,
  anchorAt,
  clampZoom,
  currentPage,
  fitZoom,
  isWithinBand,
  offsetRatio,
  pointOnPage,
  rasterRatio,
  rotated,
  scrollTopAt,
  scrollTopFor,
  stackPages,
  stackSingle,
  stepZoom,
  viewportScale,
  visibleHeight,
} from './layout.js';

/** US Letter, in points. */
const LETTER = { w: 612, h: 792 };

describe('zoom', () => {
  it('treats 100% as physical size, not one pixel per point', () => {
    expect(viewportScale(1)).toBeCloseTo(96 / 72);
  });

  it('clamps anything unusable to 1', () => {
    expect(clampZoom(Number.NaN)).toBe(1);
    expect(clampZoom(0)).toBe(1);
    expect(clampZoom(-3)).toBe(1);
  });

  it('clamps to the range', () => {
    expect(clampZoom(1000)).toBe(MAX_ZOOM);
    expect(clampZoom(0.0001)).toBe(MIN_ZOOM);
  });

  it('steps up and down through the presets', () => {
    expect(stepZoom(1, 1)).toBe(1.25);
    expect(stepZoom(1, -1)).toBe(0.8);
  });

  it('steps from a value between presets to the next one either way', () => {
    expect(stepZoom(1.1, 1)).toBe(1.25);
    expect(stepZoom(1.1, -1)).toBe(1);
  });

  it('stops at the ends rather than walking off them', () => {
    expect(stepZoom(MAX_ZOOM, 1)).toBe(MAX_ZOOM);
    expect(stepZoom(MIN_ZOOM, -1)).toBe(MIN_ZOOM);
  });
});

describe('fits', () => {
  it('fits the width to the content box, not the scroller', () => {
    const view = { w: 612 * PX_PER_PT + PAGE_PAD * 2, h: 400 };
    expect(fitZoom('fit-width', view, LETTER)).toBeCloseTo(1);
  });

  it('fits the page to whichever axis binds', () => {
    const view = { w: 10_000, h: 792 * PX_PER_PT + PAGE_PAD * 2 };
    expect(fitZoom('fit-page', view, LETTER)).toBeCloseTo(1);
  });

  it('fits the height to the height alone, whatever the width does', () => {
    const view = { w: 10_000, h: 792 * PX_PER_PT + PAGE_PAD * 2 };
    expect(fitZoom('fit-height', view, LETTER)).toBeCloseTo(1);
  });

  /**
   * The whole difference between the two: `fit-page` refuses to overflow
   * either way, `fit-height` fills the height and lets a wide page run off
   * the sides — which is the one a reader asking for a screen per page wants.
   */
  it('lets fit-height overflow sideways where fit-page will not', () => {
    const view = { w: 200, h: 792 * PX_PER_PT + PAGE_PAD * 2 };
    expect(fitZoom('fit-height', view, LETTER)).toBeCloseTo(1);
    expect(fitZoom('fit-page', view, LETTER)).toBeLessThan(1);
  });

  /**
   * A fit in a spread fits the *pair*: two pages and the gap between them
   * share the width, so the same tab that fits one page at 100% fits two at
   * rather less than half of it.
   */
  it('shares the width between the columns of a spread', () => {
    const view = { w: 612 * PX_PER_PT * 2 + PAGE_PAD * 2 + PAGE_GAP, h: 400 };
    expect(fitZoom('fit-width', view, LETTER, 2)).toBeCloseTo(1);
    expect(fitZoom('fit-width', view, LETTER, 1)).toBeCloseTo(2, 1);
  });

  it('fits the height to one page however many columns there are', () => {
    const view = { w: 400, h: 792 * PX_PER_PT + PAGE_PAD * 2 };
    expect(fitZoom('fit-height', view, LETTER, 2)).toBeCloseTo(1);
  });

  it('leaves actual size alone whatever the scroller measures', () => {
    expect(fitZoom('actual', { w: 37, h: 12 }, LETTER)).toBe(1);
  });

  it('survives a scroller that has not been laid out yet', () => {
    const zoom = fitZoom('fit-width', { w: 0, h: 0 }, LETTER);
    expect(zoom).toBeGreaterThan(0);
    expect(zoom).toBeLessThanOrEqual(MAX_ZOOM);
  });
});

describe('rotation', () => {
  it('trades width for height on a quarter turn', () => {
    expect(rotated(LETTER, 90)).toEqual({ w: 792, h: 612 });
    expect(rotated(LETTER, 270)).toEqual({ w: 792, h: 612 });
  });

  it('leaves a half turn the same shape', () => {
    expect(rotated(LETTER, 180)).toEqual(LETTER);
  });
});

describe('stacking', () => {
  it('starts at the padding and gaps between pages', () => {
    const boxes = stackPages([LETTER, LETTER, LETTER], 1);
    expect(boxes[0]!.top).toBe(PAGE_PAD);
    expect(boxes[1]!.top).toBe(PAGE_PAD + 792 + PAGE_GAP);
    expect(boxes[2]!.top).toBe(PAGE_PAD + (792 + PAGE_GAP) * 2);
  });

  it('stacks pages of different sizes correctly', () => {
    const boxes = stackPages([{ w: 100, h: 100 }, LETTER], 1);
    expect(boxes[1]!.top).toBe(PAGE_PAD + 100 + PAGE_GAP);
  });

  it('applies the rotation to every box', () => {
    const [box] = stackPages([LETTER], 1, 90);
    expect(box).toMatchObject({ w: 792, h: 612 });
  });

  it('never produces a zero-height box, however small the scale', () => {
    const [box] = stackPages([LETTER], 0.0001);
    expect(box!.h).toBeGreaterThan(0);
    expect(box!.w).toBeGreaterThan(0);
  });
});

describe('stacking two pages side by side', () => {
  it('gives the pages of a row the same top and starts the next below them', () => {
    const boxes = stackPages([LETTER, LETTER, LETTER, LETTER], 1, 0, 2);
    expect(boxes[0]!.top).toBe(PAGE_PAD);
    expect(boxes[1]!.top).toBe(PAGE_PAD);
    expect(boxes[2]!.top).toBe(PAGE_PAD + 792 + PAGE_GAP);
    expect(boxes[3]!.top).toBe(PAGE_PAD + 792 + PAGE_GAP);
  });

  /** The gap under a short page belongs to it, not to the row below. */
  it('makes a row as tall as its tallest page and no taller', () => {
    const boxes = stackPages([{ w: 100, h: 100 }, LETTER, LETTER], 1, 0, 2);
    expect(boxes[0]!.h).toBe(100);
    expect(boxes[1]!.h).toBe(792);
    expect(boxes[2]!.top).toBe(PAGE_PAD + 792 + PAGE_GAP);
  });

  it('leaves an odd last page in a row of its own', () => {
    const boxes = stackPages([LETTER, LETTER, LETTER], 1, 0, 2);
    expect(boxes).toHaveLength(3);
    expect(boxes[2]!.top).toBe(PAGE_PAD + 792 + PAGE_GAP);
  });

  it('picks the left-hand page of a row as the one being read', () => {
    const boxes = stackPages([LETTER, LETTER], 1, 0, 2);
    expect(currentPage(boxes, PAGE_PAD, 800)).toBe(1);
  });

  it('is the plain column again at one page per row', () => {
    expect(stackPages([LETTER, LETTER], 1, 0, 1)).toEqual(stackPages([LETTER, LETTER], 1));
  });
});

describe('stacking one page at a time', () => {
  const geoms = [LETTER, LETTER, LETTER];

  it('gives the shown page the whole extent and the rest nothing', () => {
    const boxes = stackSingle(geoms, 1, 1);
    expect(boxes[0]).toEqual({ w: 0, h: 0, top: 0 });
    expect(boxes[1]).toEqual({ w: 612, h: 792, top: PAGE_PAD });
    expect(boxes[2]).toEqual({ w: 0, h: 0, top: 0 });
  });

  /** Every index still means the same page, or find would step to the wrong one. */
  it('keeps a box per page so the indices still line up', () => {
    expect(stackSingle(geoms, 0, 1)).toHaveLength(3);
  });

  it('cannot be mistaken for the page the reader is on', () => {
    const boxes = stackSingle(geoms, 2, 1);
    expect(currentPage(boxes, PAGE_PAD, 800)).toBe(3);
  });
});

describe('the anchor a zoom holds still', () => {
  const boxes = stackPages([LETTER, LETTER, LETTER], 1);

  it('finds the page under a point and how far into it', () => {
    // Half way down page 2.
    const y = boxes[1]!.top + 396;
    expect(anchorAt(boxes, y, 0)).toEqual({ page: 2, ratio: 0.5 });
  });

  it('reads the point relative to the viewport, not the document', () => {
    expect(anchorAt(boxes, boxes[1]!.top, 396)).toEqual({ page: 2, ratio: 0.5 });
  });

  /** A wheel past the end of the document still has to zoom something. */
  it('falls back to the last page rather than nothing', () => {
    expect(anchorAt(boxes, 1_000_000, 0)?.page).toBe(3);
  });

  it('skips the pages single mode has taken out of the flow', () => {
    const single = stackSingle([LETTER, LETTER, LETTER], 2, 1);
    expect(anchorAt(single, 0, 0)?.page).toBe(3);
  });

  it('has no answer for a document with no pages', () => {
    expect(anchorAt([], 0, 0)).toBeNull();
  });
});

describe('the render band', () => {
  const boxes = stackPages(new Array(20).fill(LETTER), 1);

  it('includes the page on screen', () => {
    expect(isWithinBand(boxes[0]!, 0, 600)).toBe(true);
  });

  it('includes a screen of margin either way, so scrolling stays continuous', () => {
    // Page 2 starts at 824 — off a 600px screen, but inside the margin.
    expect(isWithinBand(boxes[1]!, 0, 600)).toBe(true);
  });

  it('excludes pages beyond the margin', () => {
    expect(isWithinBand(boxes[10]!, 0, 600)).toBe(false);
  });

  it('narrows to the screen when told to read no further ahead', () => {
    expect(isWithinBand(boxes[1]!, 0, 600, 0)).toBe(false);
    expect(isWithinBand(boxes[0]!, 0, 600, 0)).toBe(true);
  });
});

describe('which page the reader is on', () => {
  const boxes = stackPages([LETTER, LETTER, LETTER], 1);

  it('is the one showing the most of itself', () => {
    expect(currentPage(boxes, 0, 600)).toBe(1);
    expect(currentPage(boxes, 900, 600)).toBe(2);
  });

  it('breaks a tie towards the earlier page', () => {
    // The boundary between pages 1 and 2 sits at 808; centre the viewport there.
    const top = 808 - 300;
    expect(currentPage(boxes, top, 600)).toBe(1);
  });

  it('says nothing rather than guessing when the scroller has no height', () => {
    expect(currentPage(boxes, 0, 0)).toBeNull();
    expect(currentPage([], 0, 600)).toBeNull();
  });

  it('measures the visible slice of a page', () => {
    expect(visibleHeight(boxes[0]!, 0, 600)).toBe(600 - PAGE_PAD);
  });
});

describe('scroll anchoring', () => {
  const [box] = stackPages([LETTER], 1);

  it('puts the top of a page under the top of the viewport, less the padding', () => {
    expect(scrollTopFor(box!)).toBe(0);
  });

  it('round-trips a position within a page', () => {
    const at = scrollTopAt(box!, 0.25);
    expect(offsetRatio(box!, at)).toBeCloseTo(0.25);
  });

  it('never scrolls above the top of the document', () => {
    expect(scrollTopAt(box!, -10)).toBe(0);
  });
});

describe('a point in PDF space on the displayed page', () => {
  // PDF space puts the origin at the bottom-left; a display does not.
  it('flips the y axis when unrotated', () => {
    expect(pointOnPage(LETTER, 0, { x: 100, y: 792 }, 1)).toEqual({ x: 100, y: 0 });
    expect(pointOnPage(LETTER, 0, { x: 0, y: 0 }, 1)).toEqual({ x: 0, y: 792 });
  });

  it('maps the corners of a quarter turn', () => {
    // Top-left of the unrotated page (0, 792) becomes the top-right of a page
    // turned 90° clockwise, which is (792, 0) in a 792×612 display box.
    expect(pointOnPage(LETTER, 90, { x: 0, y: 792 }, 1)).toEqual({ x: 792, y: 0 });
    expect(pointOnPage(LETTER, 270, { x: 0, y: 792 }, 1)).toEqual({ x: 0, y: 612 });
  });

  it('rotates a half turn about the centre', () => {
    expect(pointOnPage(LETTER, 180, { x: 0, y: 0 }, 1)).toEqual({ x: 612, y: 0 });
  });

  it('applies the scale', () => {
    expect(pointOnPage(LETTER, 0, { x: 100, y: 792 }, 2)).toEqual({ x: 200, y: 0 });
  });
});

describe('the raster ratio', () => {
  it('renders at the display ratio when there is room', () => {
    expect(rasterRatio({ w: 612, h: 792, top: 0 }, 2, 16 << 20)).toBe(2);
  });

  it('goes coarse rather than asking for a bitmap the GPU will refuse', () => {
    const huge = { w: 8000, h: 10_000, top: 0 };
    const ratio = rasterRatio(huge, 3, 16 << 20);
    expect(ratio).toBeLessThan(1);
    expect(huge.w * ratio * (huge.h * ratio)).toBeLessThanOrEqual(16 << 20);
  });

  it('never rounds all the way down to nothing', () => {
    expect(rasterRatio({ w: 1e6, h: 1e6, top: 0 }, 2, 1 << 20)).toBeGreaterThan(0);
  });
});
