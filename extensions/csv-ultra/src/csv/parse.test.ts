import { describe, expect, it } from 'vitest';
import type { Dialect } from './dialect.js';
import { contentStart, fieldSpans, gridOf, parse, valuesOf } from './parse.js';

const comma: Dialect = { delimiter: ',', quote: '"', newline: '\n' };
const tab: Dialect = { delimiter: '\t', quote: '"', newline: '\n' };

const values = (text: string, dialect = comma): string[][] => gridOf(parse(text, dialect));

describe('the shape of a file', () => {
  it('reads a rectangle of plain fields', () => {
    expect(values('a,b,c\n1,2,3\n')).toEqual([
      ['a', 'b', 'c'],
      ['1', '2', '3'],
    ]);
  });

  it('has no records at all in an empty file', () => {
    const table = parse('', comma);
    expect(table.records).toEqual([]);
    expect(table.columns).toBe(0);
    expect(table.trailingNewline).toBe(false);
  });

  it('does not invent a record after the last line break', () => {
    expect(values('a\nb\n')).toEqual([['a'], ['b']]);
    expect(parse('a\nb\n', comma).trailingNewline).toBe(true);
  });

  it('keeps a file that does not end in a line break', () => {
    expect(values('a\nb')).toEqual([['a'], ['b']]);
    expect(parse('a\nb', comma).trailingNewline).toBe(false);
  });

  it('treats a blank line as a row of one empty cell', () => {
    expect(values('a\n\nb\n')).toEqual([['a'], [''], ['b']]);
  });

  it('pads short records out to the widest one', () => {
    const table = parse('a,b,c\n1\n', comma);
    expect(table.columns).toBe(3);
    expect(gridOf(table)).toEqual([
      ['a', 'b', 'c'],
      ['1', '', ''],
    ]);
    // The record itself is still one field long; only the view is padded.
    expect(table.records[1]!.fields).toHaveLength(1);
    expect(valuesOf(table.records[1]!)).toEqual(['1']);
  });

  it('reads every line ending, including a lone carriage return', () => {
    expect(values('a\r\nb\rc\n')).toEqual([['a'], ['b'], ['c']]);
  });

  it('counts a trailing delimiter as one more empty field', () => {
    expect(values('a,b,\n')).toEqual([['a', 'b', '']]);
  });

  it('uses whatever delimiter it is handed', () => {
    expect(values('a\tb\n1\t2\n', tab)).toEqual([
      ['a', 'b'],
      ['1', '2'],
    ]);
    // …and a comma is then just a character.
    expect(values('a,b\tc\n', tab)).toEqual([['a,b', 'c']]);
  });
});

describe('quoting', () => {
  it('protects delimiters, line breaks and quotes', () => {
    expect(values('"Smith, John",1\n')).toEqual([['Smith, John', '1']]);
    expect(values('"two\nlines",x\n')).toEqual([['two\nlines', 'x']]);
    expect(values('"say ""hi""",x\n')).toEqual([['say "hi"', 'x']]);
  });

  it('remembers that a field was quoted, so a rewrite can put them back', () => {
    const record = parse('"a",b\n', comma).records[0]!;
    expect(record.fields.map((field) => field.quoted)).toEqual([true, false]);
  });

  it('counts a record that spans lines as one record', () => {
    const table = parse('a,"line\nbreak"\nnext\n', comma);
    expect(table.records).toHaveLength(2);
    expect(gridOf(table)).toEqual([
      ['a', 'line\nbreak'],
      ['next', ''],
    ]);
  });

  it('swallows the rest of the file when a quote never closes', () => {
    // What a half-typed row looks like. The next keystroke fixes it; throwing
    // would blank the table in the meantime.
    expect(values('a,"unfinished\nb,c\n')).toEqual([['a', 'unfinished\nb,c\n']]);
  });

  it('keeps text that follows a closing quote instead of dropping it', () => {
    expect(values('"a"b,c\n')).toEqual([['ab', 'c']]);
  });

  it('reads an empty quoted field', () => {
    expect(values('"",x\n')).toEqual([['', 'x']]);
  });
});

describe('where a record came from', () => {
  it('spans the record and stops before its line terminator', () => {
    const text = 'a,b\r\nc,d\r\n';
    const table = parse(text, comma);
    expect(table.records.map((r) => text.slice(r.start, r.end))).toEqual(['a,b', 'c,d']);
  });

  it('spans a record that contains line breaks', () => {
    const text = 'x\n"two\nlines",y\nz\n';
    const table = parse(text, comma);
    expect(text.slice(table.records[1]!.start, table.records[1]!.end)).toBe('"two\nlines",y');
  });
});

describe('a byte-order mark', () => {
  it('is not part of the first cell', () => {
    const table = parse('﻿a,b\n', comma);
    expect(gridOf(table)).toEqual([['a', 'b']]);
    expect(table.bom).toBe(true);
    expect(contentStart(table)).toBe(1);
    expect(table.records[0]!.start).toBe(1);
  });

  it('is absent from a file that has none', () => {
    const table = parse('a\n', comma);
    expect(table.bom).toBe(false);
    expect(contentStart(table)).toBe(0);
  });

  it('leaves a file of nothing but a mark empty', () => {
    expect(parse('﻿', comma).records).toEqual([]);
  });
});

describe('where each field sits inside a record', () => {
  const spansOf = (record: string, dialect = comma): string[] =>
    fieldSpans(record, dialect).map((span) => record.slice(span.start, span.end));

  it('covers each field as written, quotes included', () => {
    expect(spansOf('a,"b,c",d')).toEqual(['a', '"b,c"', 'd']);
  });

  it('agrees with the parser about how many fields there are', () => {
    for (const record of ['a', 'a,', ',', 'a,,b', '"x""y",z', '"a"b,c']) {
      expect(fieldSpans(record, comma)).toHaveLength(parse(record, comma).records[0]!.fields.length);
    }
  });

  it('gives a blank record the one empty field the parser gives it', () => {
    // The blank line in the middle of a file: `start === end`, so the record's
    // own text is empty, and it still has a cell in it.
    expect(fieldSpans('', comma)).toEqual([{ start: 0, end: 0 }]);
    expect(parse('a\n\nb\n', comma).records[1]!.fields).toHaveLength(1);
  });

  it('gives a trailing delimiter a zero-width span after it', () => {
    expect(fieldSpans('a,', comma)).toEqual([
      { start: 0, end: 1 },
      { start: 2, end: 2 },
    ]);
  });
});
