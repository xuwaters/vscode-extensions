import type { Cell } from './selection.js';

/**
 * Reading a cell of the table, in *view* coordinates.
 *
 * Everything in this file works through this one callback rather than over an
 * array, because the view and the file are not the same table: a sorted view
 * reads row 3 out of file row 812, and the header row is in the view of one
 * reader and out of the view of another. Handing these functions the accessor
 * keeps that mapping in exactly one place — the element that owns it.
 */
export type ValueAt = (row: number, column: number) => string;

export interface FindOptions {
  query: string;
  matchCase: boolean;
  /** Match only a cell that is the query, rather than one containing it. */
  wholeCell: boolean;
}

/**
 * Every cell matching a search, in reading order.
 *
 * Collected up front rather than searched incrementally, because the count is
 * half the answer — "3 of 47" is what tells a reader whether their search was
 * the one they meant. `limit` is the guard on that: a one-letter query over a
 * million rows matches everything, and neither the count nor the list is worth
 * the second it would take.
 */
export function findMatches(
  at: ValueAt,
  rows: number,
  columns: number,
  options: FindOptions,
  limit = 50_000,
): Cell[] {
  const needle = options.matchCase ? options.query : options.query.toLowerCase();
  if (needle === '') return [];

  const matches: Cell[] = [];
  for (let row = 0; row < rows; row += 1) {
    for (let column = 0; column < columns; column += 1) {
      const raw = at(row, column);
      const value = options.matchCase ? raw : raw.toLowerCase();
      const hit = options.wholeCell ? value === needle : value.includes(needle);
      if (!hit) continue;
      matches.push({ row, column });
      if (matches.length >= limit) return matches;
    }
  }
  return matches;
}

/**
 * The match to jump to from where the reader is.
 *
 * Strictly after (or before) the current cell in reading order, wrapping round
 * the ends — so pressing Find Next from a cell that is itself a match moves on
 * rather than sitting still, and holding it down walks the whole file and comes
 * back to the start.
 */
export function nextMatch(matches: readonly Cell[], from: Cell, forward: boolean): number {
  if (matches.length === 0) return -1;
  const here = from.row * 1e9 + from.column;
  if (forward) {
    for (let index = 0; index < matches.length; index += 1) {
      const match = matches[index]!;
      if (match.row * 1e9 + match.column > here) return index;
    }
    return 0;
  }
  for (let index = matches.length - 1; index >= 0; index -= 1) {
    const match = matches[index]!;
    if (match.row * 1e9 + match.column < here) return index;
  }
  return matches.length - 1;
}

/**
 * Where ctrl-arrow lands: the edge of the run of data the reader is in.
 *
 * The spreadsheet rule, which is more useful than it first looks and is worth
 * getting exactly right because it is how anybody navigates a long file:
 *
 * * From a filled cell next to a filled cell — run to the last filled cell
 *   before a gap.
 * * From a filled cell next to a gap — skip the gap and land on the next filled
 *   cell.
 * * From a gap — land on the next filled cell, or the far edge if there is none.
 *
 * The far edge of the table is always a valid answer, so `⌃↓` in an empty column
 * goes to the bottom rather than refusing to move.
 */
export function edgeFrom(
  at: ValueAt,
  rows: number,
  columns: number,
  from: Cell,
  deltaRow: number,
  deltaColumn: number,
): Cell {
  const limit = (row: number, column: number): boolean =>
    row >= 0 && row < rows && column >= 0 && column < columns;
  if (rows === 0 || columns === 0) return from;

  const filled = (row: number, column: number): boolean => at(row, column) !== '';
  let { row, column } = from;
  const lastRow = rows - 1;
  const lastColumn = columns - 1;

  const step = (): boolean => {
    const nextRow = row + deltaRow;
    const nextColumn = column + deltaColumn;
    if (!limit(nextRow, nextColumn)) return false;
    row = nextRow;
    column = nextColumn;
    return true;
  };

  if (!limit(row + deltaRow, column + deltaColumn)) {
    return { row: clamp(row, 0, lastRow), column: clamp(column, 0, lastColumn) };
  }

  const startFilled = filled(row, column);
  const neighbourFilled = filled(row + deltaRow, column + deltaColumn);

  if (startFilled && neighbourFilled) {
    // Run to the last filled cell of this block.
    while (limit(row + deltaRow, column + deltaColumn) && filled(row + deltaRow, column + deltaColumn)) {
      step();
    }
    return { row, column };
  }

  // Skip whatever gap is ahead and land on the next filled cell; the far edge if
  // there is none.
  while (step()) {
    if (filled(row, column)) return { row, column };
  }
  return { row, column };
}

function clamp(value: number, low: number, high: number): number {
  return Math.min(high, Math.max(low, value));
}
