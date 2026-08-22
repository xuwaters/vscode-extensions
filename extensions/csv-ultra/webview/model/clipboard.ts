import { sniffDelimiter } from '../../src/csv/dialect.js';
import { gridOf, parse } from '../../src/csv/parse.js';
import { needsQuoting } from '../../src/csv/serialize.js';
import type { Rect, Selection } from './selection.js';
import type { ValueAt } from './view.js';

/**
 * What goes on the clipboard when a selection is copied.
 *
 * Tab-separated, always, whatever the file's own delimiter is — because the other
 * end of a copy is usually not this extension. Excel, Numbers, Sheets and every
 * terminal paste target read tab-separated text as a grid, and a comma-separated
 * paste into a spreadsheet lands in one column. A value holding a tab or a line
 * break is quoted, which is what those same programs expect back.
 *
 * The shape of a multi-range copy follows the spreadsheet rule rather than the
 * easy one:
 *
 * * Ranges over the same rows — three ctrl-clicked columns — are joined *across*,
 *   so the copy is those three columns side by side with nothing in between.
 * * Ranges over the same columns — four ctrl-clicked rows — are stacked.
 * * Anything else falls back to the bounding rectangle, with the cells outside
 *   the selection blank. There is no honest way to flatten an L-shape, and
 *   refusing (which is what Excel does) helps nobody.
 */
export function toDelimited(at: ValueAt, selection: Selection, delimiter = '\t'): string {
  const ranges = selection.ranges;
  if (ranges.length === 0) return '';
  if (ranges.length === 1) return block(at, ranges[0]!, delimiter);

  const sorted = [...ranges];
  if (sorted.every((rect) => rect.top === ranges[0]!.top && rect.bottom === ranges[0]!.bottom)) {
    sorted.sort((a, b) => a.left - b.left);
    return joinAcross(at, sorted, delimiter);
  }
  if (sorted.every((rect) => rect.left === ranges[0]!.left && rect.right === ranges[0]!.right)) {
    sorted.sort((a, b) => a.top - b.top);
    return sorted.map((rect) => block(at, rect, delimiter)).join('\n');
  }

  const hull = ranges.reduce((rect, other) => ({
    top: Math.min(rect.top, other.top),
    left: Math.min(rect.left, other.left),
    bottom: Math.max(rect.bottom, other.bottom),
    right: Math.max(rect.right, other.right),
  }));
  return block(
    (row, column) => (inside(ranges, row, column) ? at(row, column) : ''),
    hull,
    delimiter,
  );
}

/**
 * A pasted block, as a grid.
 *
 * The delimiter is guessed rather than assumed, because what arrives has been
 * through somebody else's hands: a spreadsheet puts tabs on the clipboard, a
 * column copied out of a CSV has commas in it, and a single value has neither.
 * `sniffDelimiter` is the same scorer the extension opens files with, so a
 * paste and an open read a block of text the same way — with quotes honoured,
 * which is what stops `"Smith, John"` becoming two cells.
 *
 * A block with exactly one cell and no line breaks is left alone entirely: a
 * value being pasted into a cell is a *value*, and splitting `3,14` into two
 * columns because it happens to hold a comma is the single most annoying thing
 * a spreadsheet paste can do.
 */
export function fromDelimited(text: string): string[][] {
  const body = text.replace(/\r\n?/g, '\n');
  if (body === '') return [['']];
  const trimmed = body.endsWith('\n') ? body.slice(0, -1) : body;

  const looksLikeGrid = /[\t\n]/.test(trimmed) || /^".*"$/s.test(trimmed);
  if (!looksLikeGrid) return [[trimmed]];

  const delimiter = /\t/.test(trimmed) ? '\t' : sniffDelimiter(trimmed);
  return gridOf(parse(trimmed, { delimiter, quote: '"', newline: '\n' }));
}

function block(at: ValueAt, rect: Rect, delimiter: string): string {
  const lines: string[] = [];
  for (let row = rect.top; row <= rect.bottom; row += 1) {
    const cells: string[] = [];
    for (let column = rect.left; column <= rect.right; column += 1) {
      cells.push(escape(at(row, column), delimiter));
    }
    lines.push(cells.join(delimiter));
  }
  return lines.join('\n');
}

function joinAcross(at: ValueAt, rects: readonly Rect[], delimiter: string): string {
  const first = rects[0]!;
  const lines: string[] = [];
  for (let row = first.top; row <= first.bottom; row += 1) {
    const cells: string[] = [];
    for (const rect of rects) {
      for (let column = rect.left; column <= rect.right; column += 1) {
        cells.push(escape(at(row, column), delimiter));
      }
    }
    lines.push(cells.join(delimiter));
  }
  return lines.join('\n');
}

function inside(ranges: readonly Rect[], row: number, column: number): boolean {
  return ranges.some(
    (rect) => row >= rect.top && row <= rect.bottom && column >= rect.left && column <= rect.right,
  );
}

function escape(value: string, delimiter: string): string {
  const dialect = { delimiter, quote: '"', newline: '\n' } as const;
  if (!needsQuoting(value, dialect)) return value;
  return `"${value.split('"').join('""')}"`;
}
