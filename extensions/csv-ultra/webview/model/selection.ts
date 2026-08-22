/**
 * What is selected, in view coordinates.
 *
 * A spreadsheet's selection is not a set of cells. It is a list of rectangles
 * with an *active* cell inside the last of them and an *anchor* that a shift
 * pivots on — which is why extending a selection then moving the cursor gives
 * back the block you started from rather than the block you dragged over.
 * Modelling it as a set of cells would be simpler and would get every one of
 * those behaviours wrong; modelling it as one rectangle would give up whole-column
 * selection, which is the thing the reader of a wide CSV wants most.
 *
 * Everything here is pure and immutable — a gesture returns a new state — so the
 * element can compare the old and the new to decide whether to repaint.
 */

export interface Cell {
  row: number;
  column: number;
}

/** A rectangle of cells. All four edges are inclusive. */
export interface Rect {
  top: number;
  left: number;
  bottom: number;
  right: number;
}

/** How the last gesture selected — what the headers highlight, and how copy joins. */
export type SelectionMode = 'cells' | 'rows' | 'columns' | 'all';

export interface Selection {
  ranges: Rect[];
  active: Cell;
  anchor: Cell;
  mode: SelectionMode;
}

/** How big the table is, so nothing can be selected outside it. */
export interface Bounds {
  rows: number;
  columns: number;
}

export function clampCell(cell: Cell, bounds: Bounds): Cell {
  return {
    row: clamp(cell.row, 0, Math.max(0, bounds.rows - 1)),
    column: clamp(cell.column, 0, Math.max(0, bounds.columns - 1)),
  };
}

/** A single cell — a plain click, and what a load starts from. */
export function atCell(cell: Cell, bounds: Bounds): Selection {
  const at = clampCell(cell, bounds);
  return {
    ranges: [{ top: at.row, left: at.column, bottom: at.row, right: at.column }],
    active: at,
    anchor: at,
    mode: 'cells',
  };
}

/**
 * Extend to a cell — shift-click, shift-arrow, or a drag.
 *
 * The *last* range is replaced rather than a new one added, and the anchor does
 * not move: dragging back and forth over a block grows and shrinks it from where
 * the gesture started, which is what makes a shift-selection correctable.
 */
export function extendTo(selection: Selection, cell: Cell, bounds: Bounds): Selection {
  const to = clampCell(cell, bounds);
  const anchor = clampCell(selection.anchor, bounds);
  const rect = rectBetween(anchor, to);
  const ranges = selection.ranges.length > 0 ? selection.ranges.slice(0, -1) : [];
  return {
    ranges: [...ranges, spanned(rect, selection.mode, bounds)],
    active: to,
    anchor,
    mode: selection.mode === 'all' ? 'cells' : selection.mode,
  };
}

/** Add a cell as a range of its own — a ctrl- or cmd-click. */
export function addCell(selection: Selection, cell: Cell, bounds: Bounds): Selection {
  const at = clampCell(cell, bounds);
  return {
    ranges: [...selection.ranges, { top: at.row, left: at.column, bottom: at.row, right: at.column }],
    active: at,
    anchor: at,
    mode: 'cells',
  };
}

/**
 * Select whole rows.
 *
 * `additive` is the ctrl-click that picks a second band without losing the first
 * — the gesture behind "delete these four rows, which are not next to each
 * other". The active cell goes to the start of the band so that typing after a
 * row selection begins where the reader is looking.
 */
export function selectRows(
  selection: Selection,
  from: number,
  to: number,
  bounds: Bounds,
  additive = false,
): Selection {
  if (bounds.rows === 0 || bounds.columns === 0) return selection;
  const top = clamp(Math.min(from, to), 0, bounds.rows - 1);
  const bottom = clamp(Math.max(from, to), 0, bounds.rows - 1);
  const rect: Rect = { top, left: 0, bottom, right: bounds.columns - 1 };
  return {
    ranges: additive ? [...selection.ranges, rect] : [rect],
    active: { row: clamp(from, 0, bounds.rows - 1), column: 0 },
    anchor: { row: clamp(from, 0, bounds.rows - 1), column: 0 },
    mode: 'rows',
  };
}

/** Select whole columns. */
export function selectColumns(
  selection: Selection,
  from: number,
  to: number,
  bounds: Bounds,
  additive = false,
): Selection {
  if (bounds.rows === 0 || bounds.columns === 0) return selection;
  const left = clamp(Math.min(from, to), 0, bounds.columns - 1);
  const right = clamp(Math.max(from, to), 0, bounds.columns - 1);
  const rect: Rect = { top: 0, left, bottom: bounds.rows - 1, right };
  return {
    ranges: additive ? [...selection.ranges, rect] : [rect],
    active: { row: 0, column: clamp(from, 0, bounds.columns - 1) },
    anchor: { row: 0, column: clamp(from, 0, bounds.columns - 1) },
    mode: 'columns',
  };
}

/** Everything. */
export function selectAll(selection: Selection, bounds: Bounds): Selection {
  if (bounds.rows === 0 || bounds.columns === 0) return selection;
  return {
    ranges: [{ top: 0, left: 0, bottom: bounds.rows - 1, right: bounds.columns - 1 }],
    active: clampCell(selection.active, bounds),
    anchor: { row: 0, column: 0 },
    mode: 'all',
  };
}

/** Whether a cell is inside the selection. Hot: called once per painted cell. */
export function contains(selection: Selection, row: number, column: number): boolean {
  for (const rect of selection.ranges) {
    if (row >= rect.top && row <= rect.bottom && column >= rect.left && column <= rect.right) {
      return true;
    }
  }
  return false;
}

/** Whether a whole row is selected — what the row header lights up for. */
export function hasRow(selection: Selection, row: number, bounds: Bounds): boolean {
  return selection.ranges.some(
    (rect) =>
      row >= rect.top && row <= rect.bottom && rect.left === 0 && rect.right >= bounds.columns - 1,
  );
}

/** Whether a whole column is selected. */
export function hasColumn(selection: Selection, column: number, bounds: Bounds): boolean {
  return selection.ranges.some(
    (rect) =>
      column >= rect.left &&
      column <= rect.right &&
      rect.top === 0 &&
      rect.bottom >= bounds.rows - 1,
  );
}

/** Whether a row is touched at all — what tints its number, short of selecting it. */
export function touchesRow(selection: Selection, row: number): boolean {
  return selection.ranges.some((rect) => row >= rect.top && row <= rect.bottom);
}

/** Whether a column is touched at all. */
export function touchesColumn(selection: Selection, column: number): boolean {
  return selection.ranges.some((rect) => column >= rect.left && column <= rect.right);
}

/** The smallest rectangle holding every range. */
export function bounding(selection: Selection): Rect {
  const first = selection.ranges[0];
  if (!first) {
    const { row, column } = selection.active;
    return { top: row, left: column, bottom: row, right: column };
  }
  let rect = { ...first };
  for (const other of selection.ranges.slice(1)) {
    rect = {
      top: Math.min(rect.top, other.top),
      left: Math.min(rect.left, other.left),
      bottom: Math.max(rect.bottom, other.bottom),
      right: Math.max(rect.right, other.right),
    };
  }
  return rect;
}

/**
 * How many cells are selected.
 *
 * Overlapping ranges are counted once — two ctrl-clicked bands that cross would
 * otherwise report more cells than the table has, and this number goes in the
 * status bar next to a sum that does count them once.
 */
export function cellCount(selection: Selection): number {
  const ranges = selection.ranges;
  if (ranges.length === 0) return 0;
  if (ranges.length === 1) return area(ranges[0]!);
  let total = 0;
  for (let index = 0; index < ranges.length; index += 1) {
    const rect = ranges[index]!;
    total += area(rect);
    // Subtract what this range shares with the ones already counted. Exact for
    // the handful of ranges a person makes by hand, and quadratic in a number
    // that never gets large.
    for (let earlier = 0; earlier < index; earlier += 1) {
      total -= area(intersect(rect, ranges[earlier]!));
    }
  }
  return Math.max(0, total);
}

/** Every selected row, ascending and without repeats. */
export function selectedRows(selection: Selection): number[] {
  const rows = new Set<number>();
  for (const rect of selection.ranges) {
    for (let row = rect.top; row <= rect.bottom; row += 1) rows.add(row);
  }
  return [...rows].sort((a, b) => a - b);
}

/** Every selected column, ascending and without repeats. */
export function selectedColumns(selection: Selection): number[] {
  const columns = new Set<number>();
  for (const rect of selection.ranges) {
    for (let column = rect.left; column <= rect.right; column += 1) columns.add(column);
  }
  return [...columns].sort((a, b) => a - b);
}

/** Re-clamp a selection to a table that changed size under it. */
export function reclamp(selection: Selection, bounds: Bounds): Selection {
  if (bounds.rows === 0 || bounds.columns === 0) {
    return { ranges: [], active: { row: 0, column: 0 }, anchor: { row: 0, column: 0 }, mode: 'cells' };
  }
  const lastRow = bounds.rows - 1;
  const lastColumn = bounds.columns - 1;
  const ranges = selection.ranges
    .filter((rect) => rect.top <= lastRow && rect.left <= lastColumn)
    .map((rect) => ({
      top: clamp(rect.top, 0, lastRow),
      left: clamp(rect.left, 0, lastColumn),
      bottom: clamp(rect.bottom, 0, lastRow),
      right: clamp(rect.right, 0, lastColumn),
    }));
  const active = clampCell(selection.active, bounds);
  return {
    ranges: ranges.length > 0 ? ranges : [{ top: active.row, left: active.column, bottom: active.row, right: active.column }],
    active,
    anchor: clampCell(selection.anchor, bounds),
    mode: selection.mode,
  };
}

function rectBetween(a: Cell, b: Cell): Rect {
  return {
    top: Math.min(a.row, b.row),
    left: Math.min(a.column, b.column),
    bottom: Math.max(a.row, b.row),
    right: Math.max(a.column, b.column),
  };
}

/**
 * Keep a row or column selection full-width or full-height as it is extended.
 *
 * Shift-clicking a second column header should select both columns entirely, not
 * the rectangle between the two cells that happened to be under the pointer.
 */
function spanned(rect: Rect, mode: SelectionMode, bounds: Bounds): Rect {
  if (mode === 'rows') return { ...rect, left: 0, right: Math.max(0, bounds.columns - 1) };
  if (mode === 'columns') return { ...rect, top: 0, bottom: Math.max(0, bounds.rows - 1) };
  return rect;
}

function area(rect: Rect | null): number {
  if (!rect) return 0;
  return (rect.bottom - rect.top + 1) * (rect.right - rect.left + 1);
}

function intersect(a: Rect, b: Rect): Rect | null {
  const rect = {
    top: Math.max(a.top, b.top),
    left: Math.max(a.left, b.left),
    bottom: Math.min(a.bottom, b.bottom),
    right: Math.min(a.right, b.right),
  };
  return rect.top <= rect.bottom && rect.left <= rect.right ? rect : null;
}

function clamp(value: number, low: number, high: number): number {
  return Math.min(high, Math.max(low, value));
}
