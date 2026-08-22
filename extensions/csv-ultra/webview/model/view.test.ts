import { describe, expect, it } from 'vitest';
import { edgeFrom, findMatches, nextMatch, type ValueAt } from './view.js';

const sheet = (rows: string[][]): { at: ValueAt; rows: number; columns: number } => ({
  at: (row, column) => rows[row]?.[column] ?? '',
  rows: rows.length,
  columns: Math.max(0, ...rows.map((row) => row.length)),
});

describe('finding', () => {
  const { at, rows, columns } = sheet([
    ['name', 'city'],
    ['Ada', 'London'],
    ['Alan', 'london'],
    ['', 'Paris'],
  ]);

  it('finds every cell containing the query, in reading order', () => {
    expect(findMatches(at, rows, columns, { query: 'lon', matchCase: false, wholeCell: false })).toEqual(
      [
        { row: 1, column: 1 },
        { row: 2, column: 1 },
      ],
    );
  });

  it('respects case when asked', () => {
    expect(
      findMatches(at, rows, columns, { query: 'London', matchCase: true, wholeCell: false }),
    ).toEqual([{ row: 1, column: 1 }]);
  });

  it('matches a whole cell when asked', () => {
    expect(findMatches(at, rows, columns, { query: 'Ada', matchCase: false, wholeCell: true })).toEqual(
      [{ row: 1, column: 0 }],
    );
    expect(findMatches(at, rows, columns, { query: 'Ad', matchCase: false, wholeCell: true })).toEqual(
      [],
    );
  });

  it('finds nothing for an empty query', () => {
    expect(findMatches(at, rows, columns, { query: '', matchCase: false, wholeCell: false })).toEqual(
      [],
    );
  });

  it('stops at the limit rather than collecting a million cells', () => {
    expect(
      findMatches(at, rows, columns, { query: 'a', matchCase: false, wholeCell: false }, 2),
    ).toHaveLength(2);
  });
});

describe('stepping between matches', () => {
  const matches = [
    { row: 0, column: 1 },
    { row: 2, column: 0 },
    { row: 5, column: 3 },
  ];

  it('goes to the next one after where the reader is', () => {
    expect(nextMatch(matches, { row: 0, column: 1 }, true)).toBe(1);
    expect(nextMatch(matches, { row: 1, column: 9 }, true)).toBe(1);
  });

  it('wraps round the end', () => {
    expect(nextMatch(matches, { row: 9, column: 0 }, true)).toBe(0);
  });

  it('goes backwards, and wraps round the start', () => {
    expect(nextMatch(matches, { row: 2, column: 0 }, false)).toBe(0);
    expect(nextMatch(matches, { row: 0, column: 0 }, false)).toBe(2);
  });

  it('has nowhere to go with no matches', () => {
    expect(nextMatch([], { row: 0, column: 0 }, true)).toBe(-1);
  });
});

describe('jumping to the edge of a block', () => {
  //   0    1    2    3
  // 0 a    b    ·    d
  // 1 e    f    ·    h
  // 2 ·    ·    ·    ·
  // 3 i    j    ·    l
  const { at, rows, columns } = sheet([
    ['a', 'b', '', 'd'],
    ['e', 'f', '', 'h'],
    ['', '', '', ''],
    ['i', 'j', '', 'l'],
  ]);
  const down = (row: number, column: number) => edgeFrom(at, rows, columns, { row, column }, 1, 0);
  const right = (row: number, column: number) => edgeFrom(at, rows, columns, { row, column }, 0, 1);

  it('runs to the last filled cell of the block it is in', () => {
    expect(down(0, 0)).toEqual({ row: 1, column: 0 });
  });

  it('skips a gap to the next filled cell', () => {
    expect(down(1, 0)).toEqual({ row: 3, column: 0 });
  });

  it('goes to the far edge when nothing is ahead', () => {
    expect(down(3, 0)).toEqual({ row: 3, column: 0 });
    expect(down(0, 2)).toEqual({ row: 3, column: 2 });
  });

  it('crosses an empty column to the next filled one', () => {
    expect(right(0, 1)).toEqual({ row: 0, column: 3 });
  });

  it('stays put at the edge of the table', () => {
    expect(right(0, 3)).toEqual({ row: 0, column: 3 });
    expect(edgeFrom(at, rows, columns, { row: 0, column: 0 }, -1, 0)).toEqual({ row: 0, column: 0 });
  });

  it('has nowhere to go in an empty table', () => {
    expect(edgeFrom(() => '', 0, 0, { row: 0, column: 0 }, 1, 0)).toEqual({ row: 0, column: 0 });
  });
});
