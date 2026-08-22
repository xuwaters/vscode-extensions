import { describe, expect, it } from 'vitest';
import {
  addCell,
  atCell,
  bounding,
  cellCount,
  clampCell,
  contains,
  extendTo,
  hasColumn,
  hasRow,
  reclamp,
  selectAll,
  selectColumns,
  selectRows,
  selectedColumns,
  selectedRows,
  touchesColumn,
  touchesRow,
} from './selection.js';

const bounds = { rows: 10, columns: 5 };

describe('a single cell', () => {
  it('is one range of one cell', () => {
    const selection = atCell({ row: 2, column: 3 }, bounds);
    expect(selection.ranges).toEqual([{ top: 2, left: 3, bottom: 2, right: 3 }]);
    expect(cellCount(selection)).toBe(1);
    expect(contains(selection, 2, 3)).toBe(true);
    expect(contains(selection, 2, 2)).toBe(false);
  });

  it('cannot land outside the table', () => {
    expect(clampCell({ row: 99, column: -4 }, bounds)).toEqual({ row: 9, column: 0 });
  });
});

describe('extending', () => {
  it('grows a block from the anchor', () => {
    const selection = extendTo(atCell({ row: 1, column: 1 }, bounds), { row: 3, column: 3 }, bounds);
    expect(selection.ranges).toEqual([{ top: 1, left: 1, bottom: 3, right: 3 }]);
    expect(selection.active).toEqual({ row: 3, column: 3 });
    expect(cellCount(selection)).toBe(9);
  });

  it('shrinks again without moving the anchor', () => {
    let selection = atCell({ row: 4, column: 4 }, bounds);
    selection = extendTo(selection, { row: 0, column: 0 }, bounds);
    selection = extendTo(selection, { row: 4, column: 3 }, bounds);
    expect(selection.ranges).toEqual([{ top: 4, left: 3, bottom: 4, right: 4 }]);
    expect(selection.anchor).toEqual({ row: 4, column: 4 });
  });

  it('extends the last range only, leaving earlier ones alone', () => {
    let selection = atCell({ row: 0, column: 0 }, bounds);
    selection = addCell(selection, { row: 5, column: 1 }, bounds);
    selection = extendTo(selection, { row: 7, column: 2 }, bounds);
    expect(selection.ranges).toEqual([
      { top: 0, left: 0, bottom: 0, right: 0 },
      { top: 5, left: 1, bottom: 7, right: 2 },
    ]);
  });
});

describe('whole rows and columns', () => {
  it('selects a band of rows across every column', () => {
    const selection = selectRows(atCell({ row: 0, column: 0 }, bounds), 2, 4, bounds);
    expect(selection.ranges).toEqual([{ top: 2, left: 0, bottom: 4, right: 4 }]);
    expect(selection.mode).toBe('rows');
    expect(hasRow(selection, 3, bounds)).toBe(true);
    expect(hasRow(selection, 5, bounds)).toBe(false);
    expect(selectedRows(selection)).toEqual([2, 3, 4]);
  });

  it('selects a band of columns down every row', () => {
    const selection = selectColumns(atCell({ row: 0, column: 0 }, bounds), 3, 1, bounds);
    expect(selection.ranges).toEqual([{ top: 0, left: 1, bottom: 9, right: 3 }]);
    expect(hasColumn(selection, 2, bounds)).toBe(true);
    expect(selectedColumns(selection)).toEqual([1, 2, 3]);
  });

  it('adds a second band without losing the first', () => {
    let selection = selectRows(atCell({ row: 0, column: 0 }, bounds), 1, 1, bounds);
    selection = selectRows(selection, 5, 6, bounds, true);
    expect(selectedRows(selection)).toEqual([1, 5, 6]);
    expect(cellCount(selection)).toBe(15);
  });

  it('keeps a shift-extended column selection full height', () => {
    let selection = selectColumns(atCell({ row: 0, column: 0 }, bounds), 1, 1, bounds);
    selection = extendTo(selection, { row: 4, column: 3 }, bounds);
    expect(selection.ranges).toEqual([{ top: 0, left: 1, bottom: 9, right: 3 }]);
  });

  it('selects everything', () => {
    const selection = selectAll(atCell({ row: 2, column: 2 }, bounds), bounds);
    expect(cellCount(selection)).toBe(50);
    // The active cell stays put, so typing after ⌘A starts where you were.
    expect(selection.active).toEqual({ row: 2, column: 2 });
  });
});

describe('counting overlapping ranges', () => {
  it('counts a shared cell once', () => {
    let selection = selectRows(atCell({ row: 0, column: 0 }, bounds), 2, 2, bounds);
    selection = selectColumns(selection, 1, 1, bounds, true);
    // A row of 5 and a column of 10 crossing at one cell.
    expect(cellCount(selection)).toBe(5 + 10 - 1);
  });

  it('counts an exact repeat once', () => {
    let selection = selectRows(atCell({ row: 0, column: 0 }, bounds), 2, 3, bounds);
    selection = selectRows(selection, 2, 3, bounds, true);
    expect(cellCount(selection)).toBe(10);
  });
});

describe('what the headers light up for', () => {
  it('separates a whole row from a row merely touched', () => {
    const selection = atCell({ row: 3, column: 1 }, bounds);
    expect(touchesRow(selection, 3)).toBe(true);
    expect(hasRow(selection, 3, bounds)).toBe(false);
    expect(touchesColumn(selection, 1)).toBe(true);
    expect(touchesColumn(selection, 2)).toBe(false);
  });
});

describe('the smallest rectangle around a selection', () => {
  it('spans every range', () => {
    let selection = atCell({ row: 1, column: 1 }, bounds);
    selection = addCell(selection, { row: 4, column: 3 }, bounds);
    expect(bounding(selection)).toEqual({ top: 1, left: 1, bottom: 4, right: 3 });
  });
});

describe('a table that changed size underneath', () => {
  it('pulls a range that hangs over the edge back inside it', () => {
    const wide = selectRows(atCell({ row: 0, column: 0 }, bounds), 2, 9, bounds);
    const narrow = reclamp(wide, { rows: 4, columns: 2 });
    expect(narrow.ranges).toEqual([{ top: 2, left: 0, bottom: 3, right: 1 }]);
    expect(narrow.active).toEqual({ row: 2, column: 0 });
  });

  it('falls back to the active cell when every range fell off', () => {
    const low = selectRows(atCell({ row: 0, column: 0 }, bounds), 8, 9, bounds);
    const narrow = reclamp(low, { rows: 4, columns: 2 });
    expect(narrow.ranges).toEqual([{ top: 3, left: 0, bottom: 3, right: 0 }]);
    expect(narrow.active).toEqual({ row: 3, column: 0 });
  });

  it('drops a range that no longer touches the table', () => {
    let selection = atCell({ row: 0, column: 0 }, bounds);
    selection = addCell(selection, { row: 9, column: 4 }, bounds);
    expect(reclamp(selection, { rows: 2, columns: 2 }).ranges).toEqual([
      { top: 0, left: 0, bottom: 0, right: 0 },
    ]);
  });

  it('survives a table with nothing in it', () => {
    const empty = reclamp(atCell({ row: 3, column: 3 }, bounds), { rows: 0, columns: 0 });
    expect(empty.ranges).toEqual([]);
    expect(cellCount(empty)).toBe(0);
  });
});
