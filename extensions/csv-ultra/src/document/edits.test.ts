import { describe, expect, it } from 'vitest';
import type { Dialect } from '../csv/dialect.js';
import { gridOf, parse } from '../csv/parse.js';
import type { QuoteStyle } from '../csv/serialize.js';
import {
  applyEdits,
  convertDialect,
  deleteColumns,
  deleteRows,
  editsFor,
  insertColumns,
  insertRows,
  setCells,
} from './edits.js';
import type { GridEdit } from '../messages.js';

const comma: Dialect = { delimiter: ',', quote: '"', newline: '\n' };
const tab: Dialect = { delimiter: '\t', quote: '"', newline: '\n' };

/** Run an edit the way the session does, and hand back the file it produced. */
const after = (
  text: string,
  edit: GridEdit,
  style: QuoteStyle = 'preserve',
  dialect = comma,
): string => applyEdits(text, editsFor(parse(text, dialect), text.length, edit, style));

describe('writing a cell', () => {
  it('rewrites one record and leaves the rest of the file alone', () => {
    const text = 'a,b\nc,d\ne,f\n';
    const edits = setCells(parse(text, comma), text.length, [{ row: 1, column: 1, value: 'X' }], 'preserve');
    expect(edits).toEqual([{ start: 4, end: 7, newText: 'c,X' }]);
    expect(applyEdits(text, edits)).toBe('a,b\nc,X\ne,f\n');
  });

  it('quotes a value that would otherwise change the shape of the row', () => {
    expect(after('a,b\n', { kind: 'cells', patches: [{ row: 0, column: 0, value: 'x,y' }] })).toBe(
      '"x,y",b\n',
    );
    expect(after('a,b\n', { kind: 'cells', patches: [{ row: 0, column: 0, value: 'two\nlines' }] })).toBe(
      '"two\nlines",b\n',
    );
  });

  it('keeps the quoting habit of the file under preserve', () => {
    expect(after('"a","b"\n', { kind: 'cells', patches: [{ row: 0, column: 0, value: 'x' }] })).toBe(
      '"x","b"\n',
    );
    expect(
      after('"a","b"\n', { kind: 'cells', patches: [{ row: 0, column: 0, value: 'x' }] }, 'minimal'),
    ).toBe('x,b\n');
  });

  it('pads a short record out to the column being written', () => {
    expect(after('a\nb\n', { kind: 'cells', patches: [{ row: 0, column: 2, value: 'z' }] })).toBe(
      'a,,z\nb\n',
    );
  });

  it('does not pad a short record to write a blank into it', () => {
    // Column 2 of a one-field record is already empty; spelling that out would
    // add delimiters the file never had.
    expect(editsFor(parse('a\n', comma), 'a\n'.length, {
      kind: 'cells',
      patches: [{ row: 0, column: 2, value: '' }],
    }, 'preserve')).toEqual([]);
  });

  it('changes nothing when the value is what was already there', () => {
    expect(
      editsFor(parse('a,b\n', comma), 'a,b\n'.length, {
        kind: 'cells',
        patches: [{ row: 0, column: 0, value: 'a' }],
      }, 'preserve'),
    ).toEqual([]);
  });

  it('groups a pasted block into one edit per row', () => {
    const text = 'a,b\nc,d\n';
    const edits = setCells(
      parse(text, comma),
      text.length,
      [
        { row: 0, column: 0, value: '1' },
        { row: 0, column: 1, value: '2' },
        { row: 1, column: 0, value: '3' },
        { row: 1, column: 1, value: '4' },
      ],
      'preserve',
    );
    expect(edits).toHaveLength(2);
    expect(applyEdits(text, edits)).toBe('1,2\n3,4\n');
  });

  it('extends the file when the paste runs off the bottom', () => {
    expect(
      after('a,b\n', {
        kind: 'cells',
        patches: [
          { row: 1, column: 0, value: 'c' },
          { row: 1, column: 1, value: 'd' },
          { row: 2, column: 0, value: 'e' },
        ],
      }),
    ).toBe('a,b\nc,d\ne\n');
  });

  it('writes into a file that has nothing in it', () => {
    expect(after('', { kind: 'cells', patches: [{ row: 0, column: 1, value: 'x' }] })).toBe(',x\n');
  });

  it('leaves a byte-order mark where it is', () => {
    const text = '﻿a,b\n';
    expect(after(text, { kind: 'cells', patches: [{ row: 0, column: 0, value: 'z' }] })).toBe(
      '﻿z,b\n',
    );
  });
});

describe('inserting rows', () => {
  it('puts a row above the one named', () => {
    expect(after('a\nb\n', { kind: 'insertRows', at: 1, rows: [['x']] })).toBe('a\nx\nb\n');
  });

  it('appends past the end, keeping the trailing newline it found', () => {
    expect(after('a\nb\n', { kind: 'insertRows', at: 9, rows: [['x']] })).toBe('a\nb\nx\n');
  });

  it('appends to a file that ends without a newline, and still does not add one', () => {
    expect(after('a\nb', { kind: 'insertRows', at: 2, rows: [['x']] })).toBe('a\nb\nx');
  });

  it('appends several at once', () => {
    expect(after('a\n', { kind: 'insertRows', at: 1, rows: [['x'], ['y']] })).toBe('a\nx\ny\n');
  });

  it('writes the first row of an empty file', () => {
    expect(after('', { kind: 'insertRows', at: 0, rows: [['x', 'y']] })).toBe('x,y\n');
  });

  it('uses the line ending the file already had', () => {
    const crlf = 'a\r\nb\r\n';
    const table = parse(crlf, { ...comma, newline: '\r\n' });
    expect(applyEdits(crlf, insertRows(table, crlf.length, 1, [['x']], 'preserve'))).toBe('a\r\nx\r\nb\r\n');
  });
});

describe('deleting rows', () => {
  it('takes the record and its terminator', () => {
    expect(after('a\nb\nc\n', { kind: 'deleteRows', rows: [1] })).toBe('a\nc\n');
  });

  it('merges a run into one edit', () => {
    const text = 'a\nb\nc\nd\n';
    const edits = deleteRows(parse(text, comma), text.length, [1, 2]);
    expect(edits).toHaveLength(1);
    expect(applyEdits(text, edits)).toBe('a\nd\n');
  });

  it('takes rows in any order, and ignores a row twice', () => {
    expect(after('a\nb\nc\nd\n', { kind: 'deleteRows', rows: [3, 1, 1] })).toBe('a\nc\n');
  });

  it('reaches back over the terminator when the last row goes', () => {
    // The file did not end with a newline, so it still must not.
    expect(after('a\nb\nc', { kind: 'deleteRows', rows: [2] })).toBe('a\nb');
  });

  it('leaves the trailing newline in place when the file had one', () => {
    expect(after('a\nb\nc\n', { kind: 'deleteRows', rows: [2] })).toBe('a\nb\n');
  });

  it('empties a file whose every row goes', () => {
    expect(after('a\nb', { kind: 'deleteRows', rows: [0, 1] })).toBe('');
  });

  it('ignores a row that is not there', () => {
    expect(deleteRows(parse('a\n', comma), 'a\n'.length, [7])).toEqual([]);
  });

  it('keeps a multi-line record whole', () => {
    expect(after('a\n"two\nlines"\nc\n', { kind: 'deleteRows', rows: [1] })).toBe('a\nc\n');
  });
});

describe('inserting columns', () => {
  it('opens a gap in every record', () => {
    expect(after('a,b\nc,d\n', { kind: 'insertColumns', at: 1, count: 1 })).toBe('a,,b\nc,,d\n');
  });

  it('inserts more than one', () => {
    expect(after('a,b\n', { kind: 'insertColumns', at: 0, count: 2 })).toBe(',,a,b\n');
  });

  it('appends when the gap is past the last column', () => {
    expect(after('a,b\n', { kind: 'insertColumns', at: 2, count: 1 })).toBe('a,b,\n');
  });

  it('leaves a record too short to reach the gap ragged', () => {
    // Row 2 has nothing at column 2 already; padding it out would add
    // delimiters the file never had.
    expect(after('a,b,c\nx\n', { kind: 'insertColumns', at: 2, count: 1 })).toBe('a,b,,c\nx\n');
  });

  it('changes nothing in a file with no records', () => {
    expect(insertColumns(parse('', comma), 0, 0, 1, 'preserve')).toEqual([]);
  });
});

describe('deleting columns', () => {
  it('removes the column from every record', () => {
    expect(after('a,b,c\n1,2,3\n', { kind: 'deleteColumns', columns: [1] })).toBe('a,c\n1,3\n');
  });

  it('removes several at once', () => {
    expect(after('a,b,c,d\n', { kind: 'deleteColumns', columns: [2, 0] })).toBe('b,d\n');
  });

  it('leaves one empty field behind rather than no record at all', () => {
    expect(after('a\nb\n', { kind: 'deleteColumns', columns: [0] })).toBe('\n\n');
  });

  it('re-quotes what no longer needs quotes and what now does', () => {
    const text = 'a,"b,c"\n';
    expect(applyEdits(text, deleteColumns(parse(text, comma), text.length, [0], 'minimal'))).toBe('"b,c"\n');
  });
});

describe('writing a sort into the file', () => {
  const text = 'name,n\ncharlie,3\nalpha,10\nbravo,2\n';

  it('sorts the body and leaves the header on top', () => {
    expect(after(text, { kind: 'sort', column: 0, direction: 'asc', header: true })).toBe(
      'name,n\nalpha,10\nbravo,2\ncharlie,3\n',
    );
  });

  it('sorts numbers as numbers', () => {
    expect(after(text, { kind: 'sort', column: 1, direction: 'asc', header: true })).toBe(
      'name,n\nbravo,2\ncharlie,3\nalpha,10\n',
    );
  });

  it('sorts the header in with the data when there is no header', () => {
    expect(after('c\na\nb\n', { kind: 'sort', column: 0, direction: 'asc', header: false })).toBe(
      'a\nb\nc\n',
    );
  });

  it('reverses', () => {
    expect(after('a\nc\nb\n', { kind: 'sort', column: 0, direction: 'desc', header: false })).toBe(
      'c\nb\na\n',
    );
  });

  it('keeps blanks at the bottom in both directions', () => {
    const ragged = 'b\n\na\n';
    expect(after(ragged, { kind: 'sort', column: 0, direction: 'asc', header: false })).toBe('a\nb\n\n');
    expect(after(ragged, { kind: 'sort', column: 0, direction: 'desc', header: false })).toBe('b\na\n\n');
  });

  it('changes nothing when the file is already in that order', () => {
    expect(
      editsFor(parse('a\nb\n', comma), 'a\nb\n'.length, {
        kind: 'sort',
        column: 0,
        direction: 'asc',
        header: false,
      }, 'preserve'),
    ).toEqual([]);
  });

  it('has nothing to do to a file of one record', () => {
    expect(
      editsFor(parse('a\n', comma), 'a\n'.length, {
        kind: 'sort',
        column: 0,
        direction: 'asc',
        header: false,
      }, 'preserve'),
    ).toEqual([]);
  });
});

describe('converting the delimiter', () => {
  it('turns CSV into TSV, re-quoting as the new delimiter demands', () => {
    const text = 'name,note\n"Smith, John",fine\nb,"has\ttab"\n';
    const converted = applyEdits(
      text,
      convertDialect(parse(text, comma), text.length, tab, 'minimal'),
    );
    expect(converted).toBe('name\tnote\nSmith, John\tfine\nb\t"has\ttab"\n');
    // The values themselves are untouched — which is the whole point.
    expect(gridOf(parse(converted, tab))).toEqual(gridOf(parse(text, comma)));
  });

  it('turns TSV back into CSV', () => {
    const text = 'a\tb\n"x,y"\tz\n';
    const back = applyEdits(text, convertDialect(parse(text, tab), text.length, comma, 'minimal'));
    expect(back).toBe('a,b\n"x,y",z\n');
  });

  it('keeps a byte-order mark and the trailing newline', () => {
    const text = '﻿a,b';
    expect(applyEdits(text, convertDialect(parse(text, comma), text.length, tab, 'minimal'))).toBe(
      '﻿a\tb',
    );
  });

  it('has nothing to do to an empty file', () => {
    expect(convertDialect(parse('', comma), 0, tab, 'minimal')).toEqual([]);
  });
});
