import { describe, expect, it } from 'vitest';
import {
  compareValues,
  isBlank,
  kindOf,
  looksLikeHeader,
  parseDate,
  parseNumber,
  summarize,
} from './values.js';

describe('reading a cell as a number', () => {
  it('reads the plain forms', () => {
    expect(parseNumber('42')).toBe(42);
    expect(parseNumber('-3.5')).toBe(-3.5);
    expect(parseNumber('+7')).toBe(7);
    expect(parseNumber('.5')).toBe(0.5);
    expect(parseNumber('  12  ')).toBe(12);
  });

  it('reads the forms a spreadsheet exports', () => {
    expect(parseNumber('1,234')).toBe(1234);
    expect(parseNumber('1 234 567')).toBe(1234567);
    expect(parseNumber('$1,234.50')).toBe(1234.5);
    expect(parseNumber('€99')).toBe(99);
    expect(parseNumber('12%')).toBe(0.12);
    expect(parseNumber('(1,234)')).toBe(-1234);
  });

  it('reads scientific notation', () => {
    expect(parseNumber('1e3')).toBe(1000);
    expect(parseNumber('-2.5e-3')).toBe(-0.0025);
  });

  it('refuses what a spreadsheet does not mean as a number', () => {
    expect(parseNumber('')).toBeNull();
    expect(parseNumber('   ')).toBeNull();
    expect(parseNumber('0x1f')).toBeNull();
    expect(parseNumber('Infinity')).toBeNull();
    expect(parseNumber('n/a')).toBeNull();
    expect(parseNumber('12 apples')).toBeNull();
    expect(parseNumber('2024-01-15')).toBeNull();
    // A bracket that never closes is not accounting notation.
    expect(parseNumber('(1234')).toBeNull();
    expect(parseNumber('1234)')).toBeNull();
  });
});

describe('reading a cell as a date', () => {
  it('reads ISO 8601 and the unambiguous slash form', () => {
    expect(parseDate('2024-01-15')).toBe(Date.UTC(2024, 0, 15));
    expect(parseDate('2024/01/15')).toBe(Date.UTC(2024, 0, 15));
    expect(parseDate('2024-01-15T09:30')).toBe(Date.UTC(2024, 0, 15, 9, 30));
    expect(parseDate('2024-01-15 09:30:15')).toBe(Date.UTC(2024, 0, 15, 9, 30, 15));
  });

  it('refuses the ambiguous ones outright', () => {
    // Third of April or fourth of March? A sort that picks one silently is
    // worse than one that sorts the column as text.
    expect(parseDate('03/04/2024')).toBeNull();
    expect(parseDate('15.01.2024')).toBeNull();
  });

  it('refuses an impossible date', () => {
    expect(parseDate('2024-13-01')).toBeNull();
    expect(parseDate('2024-01-40')).toBeNull();
  });
});

describe('what kind a cell is', () => {
  it('sorts the four cases', () => {
    expect(kindOf('')).toBe('empty');
    expect(kindOf('   ')).toBe('empty');
    expect(kindOf('12')).toBe('number');
    expect(kindOf('2024-01-15')).toBe('date');
    expect(kindOf('hello')).toBe('text');
  });

  it('agrees with isBlank about blankness', () => {
    expect(isBlank(' ')).toBe(true);
    expect(isBlank('0')).toBe(false);
  });
});

describe('comparing two cells', () => {
  const sorted = (values: string[]): string[] => [...values].sort(compareValues);

  it('orders numbers as numbers, not as strings', () => {
    expect(sorted(['10', '9', '100', '2'])).toEqual(['2', '9', '10', '100']);
  });

  it('orders dates as dates', () => {
    expect(sorted(['2024-02-01', '2023-12-31', '2024-01-15'])).toEqual([
      '2023-12-31',
      '2024-01-15',
      '2024-02-01',
    ]);
  });

  it('puts numbers before text', () => {
    expect(sorted(['zeta', '5', 'alpha', '10'])).toEqual(['5', '10', 'alpha', 'zeta']);
  });

  it('reads digits inside text naturally', () => {
    expect(sorted(['item10', 'item2', 'item1'])).toEqual(['item1', 'item2', 'item10']);
  });

  it('is a total order, so equal-looking values do not shuffle', () => {
    expect(compareValues('1.0', '1.00')).toBeLessThan(0);
    expect(compareValues('a', 'a')).toBe(0);
  });
});

describe('guessing whether the first row is a header', () => {
  it('says yes when words sit above numbers', () => {
    expect(
      looksLikeHeader([
        ['name', 'price'],
        ['apple', '1.20'],
        ['pear', '2.40'],
      ]),
    ).toBe(true);
  });

  it('says no when the first row is numbers too', () => {
    expect(
      looksLikeHeader([
        ['1', '2'],
        ['3', '4'],
      ]),
    ).toBe(false);
  });

  it('says no when a name is missing', () => {
    expect(
      looksLikeHeader([
        ['name', ''],
        ['apple', '1.20'],
      ]),
    ).toBe(false);
  });

  it('says no when two columns would share a name', () => {
    expect(
      looksLikeHeader([
        ['name', 'Name'],
        ['apple', 'pear'],
      ]),
    ).toBe(false);
  });

  it('says no about a file with nothing under the first row', () => {
    expect(looksLikeHeader([['name', 'price']])).toBe(false);
    expect(looksLikeHeader([])).toBe(false);
  });

  it('says no when every row repeats the first one', () => {
    expect(
      looksLikeHeader([
        ['a', 'b'],
        ['a', 'b'],
      ]),
    ).toBe(false);
  });
});

describe('summarising a selection', () => {
  it('counts, totals and bounds the numbers in it', () => {
    const summary = summarize(['1', '2', '3', 'x', '']);
    expect(summary).toMatchObject({ cells: 5, filled: 4, numeric: 3, sum: 6, min: 1, max: 3 });
  });

  it('leaves the bounds unset when nothing is numeric', () => {
    const summary = summarize(['a', 'b']);
    expect(summary.numeric).toBe(0);
    expect(summary.min).toBe(Number.POSITIVE_INFINITY);
  });

  it('summarises nothing at all', () => {
    expect(summarize([]).cells).toBe(0);
  });
});
