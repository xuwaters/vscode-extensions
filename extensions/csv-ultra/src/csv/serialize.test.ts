import { describe, expect, it } from 'vitest';
import type { Dialect } from './dialect.js';
import { gridOf, parse } from './parse.js';
import {
  fieldsOf,
  needsQuoting,
  writeField,
  writeRecord,
  writeTable,
  writeValues,
} from './serialize.js';

const comma: Dialect = { delimiter: ',', quote: '"', newline: '\n' };
const tab: Dialect = { delimiter: '\t', quote: '"', newline: '\n' };
const crlf: Dialect = { delimiter: ',', quote: '"', newline: '\r\n' };

describe('what has to be quoted', () => {
  it('is the delimiter, the quote, a line break, and edge whitespace', () => {
    expect(needsQuoting('a,b', comma)).toBe(true);
    expect(needsQuoting('say "hi"', comma)).toBe(true);
    expect(needsQuoting('two\nlines', comma)).toBe(true);
    expect(needsQuoting(' padded', comma)).toBe(true);
    expect(needsQuoting('padded ', comma)).toBe(true);
  });

  it('is not a delimiter that belongs to another dialect', () => {
    expect(needsQuoting('a,b', tab)).toBe(false);
    expect(needsQuoting('a\tb', comma)).toBe(false);
  });

  it('is nothing about an ordinary value', () => {
    expect(needsQuoting('plain', comma)).toBe(false);
    expect(needsQuoting('', comma)).toBe(false);
  });
});

describe('writing a field', () => {
  it('quotes what has to be quoted whatever the style', () => {
    for (const style of ['minimal', 'preserve', 'always'] as const) {
      expect(writeField({ value: 'a,b', quoted: false }, comma, style)).toBe('"a,b"');
    }
  });

  it('doubles a quote inside a quoted value', () => {
    expect(writeField({ value: 'say "hi"', quoted: false }, comma, 'minimal')).toBe('"say ""hi"""');
  });

  it('leaves an ordinary value bare under minimal', () => {
    expect(writeField({ value: 'plain', quoted: true }, comma, 'minimal')).toBe('plain');
  });

  it('keeps the quotes the file had under preserve', () => {
    expect(writeField({ value: 'plain', quoted: true }, comma, 'preserve')).toBe('"plain"');
    expect(writeField({ value: 'plain', quoted: false }, comma, 'preserve')).toBe('plain');
  });

  it('quotes everything under always', () => {
    expect(writeField({ value: '', quoted: false }, comma, 'always')).toBe('""');
  });
});

describe('writing a record', () => {
  it('joins with the delimiter it is given', () => {
    expect(writeValues(['a', 'b', 'c'], comma, 'minimal')).toBe('a,b,c');
    expect(writeValues(['a', 'b'], tab, 'minimal')).toBe('a\tb');
  });

  it('writes an empty record as nothing at all', () => {
    expect(writeValues([''], comma, 'minimal')).toBe('');
  });

  it('carries a value holding another dialect delimiter through unquoted', () => {
    expect(writeValues(['a,b'], tab, 'minimal')).toBe('a,b');
  });
});

describe('writing a whole file', () => {
  const table = parse('a,b\n1,2\n', comma);

  it('round-trips a file it did not change', () => {
    expect(writeTable(table.records, comma, 'preserve', table.trailingNewline)).toBe('a,b\n1,2\n');
  });

  it('honours the newline it is given', () => {
    expect(writeTable(table.records, crlf, 'preserve', true)).toBe('a,b\r\n1,2\r\n');
  });

  it('omits the final terminator for a file that had none', () => {
    expect(writeTable(table.records, comma, 'preserve', false)).toBe('a,b\n1,2');
  });

  it('writes nothing for a file with no records', () => {
    expect(writeTable([], comma, 'preserve', true)).toBe('');
  });

  it('converts a dialect without disturbing the values', () => {
    const source = 'name,note\n"Smith, John",ok\nb,"has\ttab"\n';
    const before = parse(source, comma);
    const converted = writeTable(before.records, tab, 'minimal', before.trailingNewline);
    // Values survive; what used to need quotes no longer does, and what did not
    // need them now does.
    expect(converted).toBe('name\tnote\nSmith, John\tok\nb\t"has\ttab"\n');
    expect(gridOf(parse(converted, tab))).toEqual(gridOf(before));
  });
});

describe('building fields from plain values', () => {
  it('marks them unquoted by default', () => {
    expect(fieldsOf(['a'])).toEqual([{ value: 'a', quoted: false }]);
  });

  it('can mark them quoted, for a file that quotes everything', () => {
    expect(writeRecord(fieldsOf(['a'], true), comma, 'preserve')).toBe('"a"');
  });
});
