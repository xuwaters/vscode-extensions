import { describe, expect, it } from 'vitest';
import { destinationPoint, resolveDestination } from './destinations.js';

/** A page reference, as pdf.js hands one over. */
const ref = { num: 12, gen: 0 };

describe('the point a destination names', () => {
  it('reads left and top out of an /XYZ destination', () => {
    expect(destinationPoint([ref, { name: 'XYZ' }, 100, 700, null])).toEqual({
      x: 100,
      y: 700,
    });
  });

  it('reads the top edge out of the horizontal fits', () => {
    expect(destinationPoint([ref, { name: 'FitH' }, 700])).toEqual({ x: 0, y: 700 });
    expect(destinationPoint([ref, { name: 'FitBH' }, 700])).toEqual({ x: 0, y: 700 });
  });

  it('names no point for a destination that describes a fit, not a position', () => {
    // `/Fit` and `/FitV` say how to scale, not where to look — the top of the
    // page is the honest answer rather than a number made up from the pieces.
    expect(destinationPoint([ref, { name: 'Fit' }])).toBeNull();
    expect(destinationPoint([ref, { name: 'FitV' }, 100])).toBeNull();
    expect(destinationPoint([ref, { name: 'FitR' }, 0, 0, 100, 100])).toBeNull();
  });

  it('treats a null top as "keep the current view"', () => {
    // `/XYZ null null null` is legal and means exactly that.
    expect(destinationPoint([ref, { name: 'XYZ' }, null, null, null])).toBeNull();
  });

  it('defaults a missing left to the left edge', () => {
    expect(destinationPoint([ref, { name: 'XYZ' }, null, 700, null])).toEqual({
      x: 0,
      y: 700,
    });
  });

  it('names no point for a destination with no view specification at all', () => {
    expect(destinationPoint([ref])).toBeNull();
    expect(destinationPoint([ref, 'nonsense'])).toBeNull();
  });
});

describe('resolving a destination against a document', () => {
  const doc = {
    getDestination: async (name: string) =>
      name === 'chapter-2' ? [ref, { name: 'XYZ' }, 0, 500, null] : null,
    getPageIndex: async (target: unknown) => {
      if (target !== ref) throw new Error('no such page');
      return 6;
    },
  } as never;

  it('turns a named destination into a 1-based page and a point', async () => {
    expect(await resolveDestination(doc, 'chapter-2')).toEqual({
      page: 7,
      point: { x: 0, y: 500 },
    });
  });

  it('takes an inline destination array as it stands', async () => {
    expect(await resolveDestination(doc, [ref, { name: 'Fit' }])).toEqual({
      page: 7,
      point: null,
    });
  });

  it('resolves nothing for a link that goes nowhere, rather than throwing', async () => {
    // A malformed link should do nothing when clicked, not throw out of a click.
    expect(await resolveDestination(doc, null)).toBeUndefined();
    expect(await resolveDestination(doc, undefined)).toBeUndefined();
    expect(await resolveDestination(doc, 'no-such-name')).toBeUndefined();
    expect(await resolveDestination(doc, [])).toBeUndefined();
    expect(await resolveDestination(doc, [{ num: 99, gen: 0 }])).toBeUndefined();
  });
});
