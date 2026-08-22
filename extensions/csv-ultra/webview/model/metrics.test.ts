import { describe, expect, it } from 'vitest';
import { ColumnMetrics, RowMetrics } from './metrics.js';

describe('column widths', () => {
  const metrics = (): ColumnMetrics => {
    const columns = new ColumnMetrics(100);
    columns.setCount(5);
    return columns;
  };

  it('lays every column out at the default width', () => {
    const columns = metrics();
    expect(columns.offset(0)).toBe(0);
    expect(columns.offset(3)).toBe(300);
    expect(columns.total()).toBe(500);
  });

  it('shifts everything after a column that was dragged', () => {
    const columns = metrics();
    columns.setWidth(1, 250);
    expect(columns.offset(1)).toBe(100);
    expect(columns.offset(2)).toBe(350);
    expect(columns.total()).toBe(650);
  });

  it('clamps a width to the range it allows', () => {
    const columns = new ColumnMetrics(100, 40, 400);
    columns.setCount(2);
    columns.setWidth(0, 5);
    expect(columns.width(0)).toBe(40);
    columns.setWidth(0, 5000);
    expect(columns.width(0)).toBe(400);
  });

  it('finds the column an offset falls in', () => {
    const columns = metrics();
    columns.setWidth(0, 200);
    expect(columns.indexAt(0)).toBe(0);
    expect(columns.indexAt(199)).toBe(0);
    expect(columns.indexAt(200)).toBe(1);
    expect(columns.indexAt(305)).toBe(2);
  });

  it('clamps an offset past either end into the table', () => {
    const columns = metrics();
    expect(columns.indexAt(-50)).toBe(0);
    expect(columns.indexAt(99999)).toBe(4);
  });

  it('reports the visible slice', () => {
    const columns = metrics();
    expect(columns.window(150, 320)).toEqual({ first: 1, last: 3 });
  });

  it('has no visible slice with no columns', () => {
    expect(new ColumnMetrics(100).window(0, 500)).toEqual({ first: 0, last: -1 });
  });

  it('carries only the dragged widths into the layout', () => {
    const columns = metrics();
    columns.setWidth(3, 180);
    columns.setWidth(1, 120);
    expect(columns.sizes()).toEqual([
      [1, 120],
      [3, 180],
    ]);
    expect(columns.isSized(2)).toBe(false);
  });

  it('restores a layout, and forgets one', () => {
    const columns = metrics();
    columns.restore([
      [0, 60],
      [4, 300],
    ]);
    expect(columns.total()).toBe(60 + 100 + 100 + 100 + 300);
    columns.clear();
    expect(columns.total()).toBe(500);
  });

  it('reflows when the default width changes', () => {
    const columns = metrics();
    columns.setDefault(50);
    expect(columns.total()).toBe(250);
  });
});

describe('row heights', () => {
  const metrics = (count = 1000, height = 20): RowMetrics => {
    const rows = new RowMetrics(height);
    rows.setCount(count);
    return rows;
  };

  it('is arithmetic while every row is the same', () => {
    const rows = metrics();
    expect(rows.offset(0)).toBe(0);
    expect(rows.offset(500)).toBe(10_000);
    expect(rows.total()).toBe(20_000);
    expect(rows.indexAt(10_000)).toBe(500);
    expect(rows.indexAt(10_019)).toBe(500);
  });

  it('shifts everything below a row that was dragged', () => {
    const rows = metrics();
    rows.setHeight(10, 100);
    expect(rows.offset(10)).toBe(200);
    expect(rows.offset(11)).toBe(300);
    expect(rows.total()).toBe(20_000 + 80);
  });

  it('finds a dragged row, and the rows either side of it', () => {
    const rows = metrics();
    rows.setHeight(10, 100);
    expect(rows.indexAt(199)).toBe(9);
    expect(rows.indexAt(200)).toBe(10);
    expect(rows.indexAt(299)).toBe(10);
    expect(rows.indexAt(300)).toBe(11);
    expect(rows.indexAt(320)).toBe(12);
  });

  it('agrees with itself about every row, however many are dragged', () => {
    // The property that matters: the row an offset falls in is the row whose
    // own span contains it. Checked exhaustively rather than by example,
    // because `indexAt` is called with a scroll offset on every frame and being
    // one row out is a table that jumps.
    const rows = metrics(60, 20);
    for (const [row, height] of [
      [0, 55],
      [3, 12],
      [4, 80],
      [40, 200],
      [59, 33],
    ] as const) {
      rows.setHeight(row, height);
    }
    for (let row = 0; row < 60; row += 1) {
      const top = rows.offset(row);
      expect(rows.indexAt(top)).toBe(row);
      expect(rows.indexAt(top + rows.height(row) - 1)).toBe(row);
      expect(rows.offset(row + 1)).toBe(top + rows.height(row));
    }
    expect(rows.offset(60)).toBe(rows.total());
  });

  it('clamps an offset past either end into the table', () => {
    const rows = metrics(10);
    expect(rows.indexAt(-100)).toBe(0);
    expect(rows.indexAt(1_000_000)).toBe(9);
  });

  it('gives a row its default height back', () => {
    const rows = metrics(10);
    rows.setHeight(2, 90);
    expect(rows.total()).toBe(270);
    rows.reset(2);
    expect(rows.total()).toBe(200);
    expect(rows.isSized(2)).toBe(false);
  });

  it('reflows when the default height changes', () => {
    const rows = metrics(10);
    rows.setHeight(0, 100);
    rows.setDefault(40);
    expect(rows.offset(1)).toBe(100);
    expect(rows.total()).toBe(100 + 9 * 40);
  });

  it('keeps a shrunk file out of its own height', () => {
    const rows = metrics(10);
    rows.setHeight(9, 200);
    expect(rows.total()).toBe(9 * 20 + 200);
    rows.setCount(5);
    expect(rows.total()).toBe(100);
    // …and gives it back when the rows come back, which is what an undo does.
    rows.setCount(10);
    expect(rows.total()).toBe(9 * 20 + 200);
  });

  it('has no visible slice with no rows', () => {
    expect(new RowMetrics(20).window(0, 500)).toEqual({ first: 0, last: -1 });
  });

  it('answers instantly for a very large table', () => {
    const rows = metrics(5_000_000);
    rows.setHeight(4_000_000, 300);
    expect(rows.offset(4_000_001)).toBe(4_000_000 * 20 + 300);
    expect(rows.indexAt(4_000_000 * 20 + 150)).toBe(4_000_000);
    expect(rows.total()).toBe(5_000_000 * 20 + 280);
  });
});
