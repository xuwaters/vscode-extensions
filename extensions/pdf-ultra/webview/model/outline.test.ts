import { describe, expect, it } from 'vitest';
import {
  MAX_OUTLINE_DEPTH,
  flattenOutline,
  rowForPage,
  visibleRows,
  type RawOutlineItem,
} from './outline.js';

const tree: RawOutlineItem[] = [
  {
    title: 'One',
    items: [{ title: 'One.a', items: [{ title: 'One.a.i' }] }, { title: 'One.b' }],
  },
  { title: 'Two' },
];

describe('flattening', () => {
  it('is depth-first, in reading order', () => {
    expect(flattenOutline(tree).map((row) => row.title)).toEqual([
      'One',
      'One.a',
      'One.a.i',
      'One.b',
      'Two',
    ]);
  });

  it('records how deep each row sits', () => {
    expect(flattenOutline(tree).map((row) => row.depth)).toEqual([0, 1, 2, 1, 0]);
  });

  it('gives a parent its whole subtree, not just its first level', () => {
    const rows = flattenOutline(tree);
    expect(rows[0]!.children).toEqual(['o1', 'o2', 'o3']);
    expect(rows[1]!.children).toEqual(['o2']);
    expect(rows[4]!.children).toEqual([]);
  });

  it('tidies whitespace out of a title and names the untitled', () => {
    const rows = flattenOutline([{ title: '  a \n b  ' }, { title: '' }]);
    expect(rows.map((row) => row.title)).toEqual(['a b', 'Untitled']);
  });

  it('handles a document with no outline at all', () => {
    expect(flattenOutline(null)).toEqual([]);
    expect(flattenOutline(undefined)).toEqual([]);
  });
});

describe('flattening a hostile outline', () => {
  // A PDF is an untrusted input, and its outline is a graph the format does not
  // promise is a tree.

  it('does not recurse forever around a cycle', () => {
    const loop: RawOutlineItem = { title: 'Loop' };
    loop.items = [loop];
    expect(flattenOutline([loop]).map((row) => row.title)).toEqual(['Loop']);
  });

  it('shares a node between two parents without duplicating it', () => {
    const shared: RawOutlineItem = { title: 'Shared' };
    const rows = flattenOutline([
      { title: 'A', items: [shared] },
      { title: 'B', items: [shared] },
    ]);
    expect(rows.map((row) => row.title)).toEqual(['A', 'Shared', 'B']);
  });

  it('stops at the row limit', () => {
    const many = Array.from({ length: 50 }, (_, i) => ({ title: `#${i}` }));
    expect(flattenOutline(many, 10)).toHaveLength(10);
  });

  it('stops descending past the depth limit', () => {
    let deepest: RawOutlineItem = { title: 'bottom' };
    for (let i = 0; i < MAX_OUTLINE_DEPTH + 5; i += 1) {
      deepest = { title: `level ${i}`, items: [deepest] };
    }
    const rows = flattenOutline([deepest]);
    expect(Math.max(...rows.map((row) => row.depth))).toBeLessThanOrEqual(
      MAX_OUTLINE_DEPTH,
    );
  });
});

describe('collapsing', () => {
  const rows = flattenOutline(tree);

  it('shows everything with nothing collapsed', () => {
    expect(visibleRows(rows, new Set())).toHaveLength(5);
  });

  it('hides the whole subtree under a collapsed row', () => {
    expect(visibleRows(rows, new Set(['o0'])).map((row) => row.title)).toEqual([
      'One',
      'Two',
    ]);
  });

  it('hides only what is below the collapsed row', () => {
    expect(visibleRows(rows, new Set(['o1'])).map((row) => row.title)).toEqual([
      'One',
      'One.a',
      'One.b',
      'Two',
    ]);
  });
});

describe('which row is current', () => {
  const pages = [1, 3, undefined, 8];

  it('is the last entry at or before the page being read', () => {
    expect(rowForPage(pages, 1)).toBe(0);
    expect(rowForPage(pages, 5)).toBe(1);
    expect(rowForPage(pages, 9)).toBe(3);
  });

  it('is none when the reader is above the first entry', () => {
    expect(rowForPage([5, 9], 1)).toBe(-1);
  });

  it('ignores entries whose destination has not resolved yet', () => {
    expect(rowForPage([undefined, undefined], 4)).toBe(-1);
  });
});
