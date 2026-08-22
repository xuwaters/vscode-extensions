import { describe, expect, it } from 'vitest';
import type { GridLayout } from '../messages.js';
import { LAYOUTS_LIMIT, withLayout, type StoredLayouts } from './layoutMemory.js';

const layout = (activeRow: number): GridLayout => ({
  widths: [],
  heights: [],
  sort: null,
  header: null,
  wrap: null,
  fontSize: null,
  scrollTop: 0,
  scrollLeft: 0,
  activeRow,
  activeColumn: 0,
});

describe('remembering how a file was laid out', () => {
  it('records one', () => {
    const stored = withLayout({}, 'file:///a.csv', layout(3), 100);
    expect(stored['file:///a.csv']).toEqual({ layout: layout(3), at: 100 });
  });

  it('replaces the entry for a file it already knew', () => {
    let stored = withLayout({}, 'a', layout(1), 100);
    stored = withLayout(stored, 'a', layout(9), 200);
    expect(Object.keys(stored)).toEqual(['a']);
    expect(stored['a']?.layout.activeRow).toBe(9);
  });

  it('evicts the least recently opened past the limit', () => {
    let stored: StoredLayouts = {};
    for (let index = 0; index < LAYOUTS_LIMIT; index += 1) {
      stored = withLayout(stored, `file${index}`, layout(index), index);
    }
    stored = withLayout(stored, 'newest', layout(0), 99_999);
    expect(Object.keys(stored)).toHaveLength(LAYOUTS_LIMIT);
    expect(stored['file0']).toBeUndefined();
    expect(stored['file1']).toBeDefined();
    expect(stored['newest']).toBeDefined();
  });

  it('never evicts the file being read, whatever the clock says', () => {
    let stored: StoredLayouts = {};
    for (let index = 0; index < LAYOUTS_LIMIT; index += 1) {
      stored = withLayout(stored, `file${index}`, layout(index), 1000 + index);
    }
    // A clock that went backwards: the newest entry has the oldest timestamp.
    stored = withLayout(stored, 'current', layout(0), 0);
    expect(stored['current']).toBeDefined();
    expect(Object.keys(stored)).toHaveLength(LAYOUTS_LIMIT);
  });
});
