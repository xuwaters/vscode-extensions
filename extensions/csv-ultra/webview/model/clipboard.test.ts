import { describe, expect, it } from 'vitest';
import { fromDelimited, toDelimited } from './clipboard.js';
import {
  addCell,
  atCell,
  extendTo,
  selectColumns,
  selectRows,
  type Selection,
} from './selection.js';
import type { ValueAt } from './view.js';

const rows = [
  ['a1', 'b1', 'c1'],
  ['a2', 'b2', 'c2'],
  ['a3', 'b3', 'c3'],
];
const at: ValueAt = (row, column) => rows[row]?.[column] ?? '';
const bounds = { rows: 3, columns: 3 };

describe('copying a selection', () => {
  it('writes a rectangle as tab-separated lines', () => {
    const selection = extendTo(atCell({ row: 0, column: 0 }, bounds), { row: 1, column: 1 }, bounds);
    expect(toDelimited(at, selection)).toBe('a1\tb1\na2\tb2');
  });

  it('writes a single cell as its value', () => {
    expect(toDelimited(at, atCell({ row: 2, column: 2 }, bounds))).toBe('c3');
  });

  it('quotes a value holding a tab or a line break', () => {
    const awkward: ValueAt = () => 'has\ttab\nand break';
    expect(toDelimited(awkward, atCell({ row: 0, column: 0 }, bounds))).toBe(
      '"has\ttab\nand break"',
    );
  });

  it('joins ctrl-clicked columns side by side, skipping what is between', () => {
    let selection = selectColumns(atCell({ row: 0, column: 0 }, bounds), 0, 0, bounds);
    selection = selectColumns(selection, 2, 2, bounds, true);
    expect(toDelimited(at, selection)).toBe('a1\tc1\na2\tc2\na3\tc3');
  });

  it('stacks ctrl-clicked rows, skipping what is between', () => {
    let selection = selectRows(atCell({ row: 0, column: 0 }, bounds), 0, 0, bounds);
    selection = selectRows(selection, 2, 2, bounds, true);
    expect(toDelimited(at, selection)).toBe('a1\tb1\tc1\na3\tb3\tc3');
  });

  it('takes the bounding block of an L-shape, blanking what is outside it', () => {
    let selection = atCell({ row: 0, column: 0 }, bounds);
    selection = addCell(selection, { row: 1, column: 1 }, bounds);
    expect(toDelimited(at, selection)).toBe('a1\t\n\tb2');
  });

  it('writes nothing for a selection of nothing', () => {
    const empty: Selection = {
      ranges: [],
      active: { row: 0, column: 0 },
      anchor: { row: 0, column: 0 },
      mode: 'cells',
    };
    expect(toDelimited(at, empty)).toBe('');
  });

  it('can be asked for another delimiter', () => {
    const selection = extendTo(atCell({ row: 0, column: 0 }, bounds), { row: 0, column: 1 }, bounds);
    expect(toDelimited(at, selection, ',')).toBe('a1,b1');
  });
});

describe('pasting a block', () => {
  it('reads what a spreadsheet puts on the clipboard', () => {
    expect(fromDelimited('a\tb\n1\t2')).toEqual([
      ['a', 'b'],
      ['1', '2'],
    ]);
  });

  it('reads a comma-separated block, quotes and all', () => {
    expect(fromDelimited('name,city\n"Smith, John",London')).toEqual([
      ['name', 'city'],
      ['Smith, John', 'London'],
    ]);
  });

  it('leaves a single value alone even when it holds a comma', () => {
    // Splitting `3,14` into two columns is the most annoying thing a paste can
    // do, and a value with no tab and no line break is a value.
    expect(fromDelimited('3,14')).toEqual([['3,14']]);
    expect(fromDelimited('hello')).toEqual([['hello']]);
  });

  it('ignores a trailing newline', () => {
    expect(fromDelimited('a\tb\n')).toEqual([['a', 'b']]);
  });

  it('reads Windows line endings', () => {
    expect(fromDelimited('a\tb\r\nc\td\r\n')).toEqual([
      ['a', 'b'],
      ['c', 'd'],
    ]);
  });

  it('reads a single quoted value as one cell', () => {
    expect(fromDelimited('"two\nlines"')).toEqual([['two\nlines']]);
  });

  it('pastes nothing as one empty cell', () => {
    expect(fromDelimited('')).toEqual([['']]);
  });
});
