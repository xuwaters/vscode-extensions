import { describe, expect, it } from 'vitest';
import {
  delimiterForPath,
  delimiterName,
  newlineOf,
  resolveDialect,
  sniffDelimiter,
} from './dialect.js';

describe('the delimiter a file name settles', () => {
  it('knows the four extensions that say so outright', () => {
    expect(delimiterForPath('/tmp/data.tsv')).toBe('\t');
    expect(delimiterForPath('/tmp/data.TAB')).toBe('\t');
    expect(delimiterForPath('/tmp/data.psv')).toBe('|');
    expect(delimiterForPath('/tmp/data.csv')).toBe(',');
  });

  it('has no opinion about anything else', () => {
    expect(delimiterForPath('/tmp/data.txt')).toBeUndefined();
    expect(delimiterForPath('/tmp/data')).toBeUndefined();
  });
});

describe('sniffing a delimiter out of the content', () => {
  it('finds the character that cuts the file into equal rows', () => {
    expect(sniffDelimiter('a,b,c\n1,2,3\n4,5,6\n')).toBe(',');
    expect(sniffDelimiter('a\tb\tc\n1\t2\t3\n')).toBe('\t');
    expect(sniffDelimiter('a;b;c\n1;2;3\n')).toBe(';');
    expect(sniffDelimiter('a|b|c\n1|2|3\n')).toBe('|');
  });

  it('ignores a character that only appears inside quotes', () => {
    const text = '"Smith, John";42\n"Doe, Jane";43\n"Roe, Bob";44\n';
    expect(sniffDelimiter(text)).toBe(';');
  });

  it('prefers the consistent character over the merely frequent one', () => {
    // Commas are scattered through the prose and never line up; the tab is the
    // one that gives every record the same shape.
    const text =
      'title\tnotes\n' +
      'one\there, there, and there\n' +
      'two\tsomething, else\n' +
      'three\ta, b, c, d\n';
    expect(sniffDelimiter(text)).toBe('\t');
  });

  it('settles on a comma when nothing divides the file', () => {
    expect(sniffDelimiter('one\ntwo\nthree\n')).toBe(',');
    expect(sniffDelimiter('')).toBe(',');
  });

  it('reads a single line with no trailing break', () => {
    expect(sniffDelimiter('a\tb\tc')).toBe('\t');
  });

  it('is not fooled by a ragged tail in a long file', () => {
    const body = Array.from({ length: 60 }, (_, i) => `${i},${i * 2},${i * 3}`).join('\n');
    expect(sniffDelimiter(`${body}\nleftover`)).toBe(',');
  });
});

describe('the line ending a file already uses', () => {
  it('takes the first one it finds', () => {
    expect(newlineOf('a\r\nb\n')).toBe('\r\n');
    expect(newlineOf('a\nb\r\n')).toBe('\n');
  });

  it('falls back when the file has no line break at all', () => {
    expect(newlineOf('a,b')).toBe('\n');
    expect(newlineOf('a,b', '\r\n')).toBe('\r\n');
  });
});

describe('resolving one dialect for one document', () => {
  it('lets a configured delimiter beat both the name and the content', () => {
    const dialect = resolveDialect({ configured: ';', path: '/x/data.tsv', text: 'a,b\n' });
    expect(dialect.delimiter).toBe(';');
  });

  it('lets the file name beat the content', () => {
    const dialect = resolveDialect({ configured: 'auto', path: '/x/data.tsv', text: 'a,b,c\n' });
    expect(dialect.delimiter).toBe('\t');
  });

  it('falls through to the content for a name that says nothing', () => {
    const dialect = resolveDialect({ configured: 'auto', path: '/x/data.txt', text: 'a;b\nc;d\n' });
    expect(dialect.delimiter).toBe(';');
  });

  it('carries the line ending the file already uses', () => {
    expect(resolveDialect({ configured: 'auto', path: 'a.csv', text: 'a\r\n' }).newline).toBe('\r\n');
  });
});

describe('naming a delimiter for a person', () => {
  it('spells out the punctuation', () => {
    expect(delimiterName('\t')).toBe('Tab');
    expect(delimiterName(',')).toBe('Comma');
  });

  it('falls back to the character itself', () => {
    expect(delimiterName('~')).toBe('~');
  });
});
