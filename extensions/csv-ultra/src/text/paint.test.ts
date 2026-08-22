import { describe, expect, it } from 'vitest';
import type { Dialect } from '../csv/dialect.js';
import { parse } from '../csv/parse.js';
import { columnLabel } from '../csv/values.js';
import { paintPlan, RAINBOW_COLORS } from './paint.js';

const comma: Dialect = { delimiter: ',', quote: '"', newline: '\n' };

/** What the decorator paints, as the text under each colour. */
function painted(text: string, from = 0, to = 1e9, colors = RAINBOW_COLORS): string[][] {
  const table = parse(text, comma);
  const plan = paintPlan(
    table,
    from,
    to,
    (index) => {
      const record = table.records[index]!;
      return text.slice(record.start, record.end);
    },
    colors,
  );
  return plan.map((spans) => spans.map((span) => text.slice(span.start, span.end)));
}

describe('colouring a file by column', () => {
  it('gives each column its own colour, cycling through the palette', () => {
    const plan = painted('a,b,c\n1,2,3\n');
    expect(plan[0]).toEqual(['a', '1']);
    expect(plan[1]).toEqual(['b', '2']);
    expect(plan[2]).toEqual(['c', '3']);
    expect(plan[3]).toEqual([]);
  });

  it('wraps round the palette, so column 11 matches column 1', () => {
    const wide = Array.from({ length: 12 }, (_, i) => `c${i}`).join(',');
    const plan = painted(`${wide}\n`);
    expect(plan[0]).toEqual(['c0', 'c10']);
    expect(plan[1]).toEqual(['c1', 'c11']);
  });

  it('covers a quoted field including its quotes', () => {
    // The paint has to sit over what the reader sees, not over the value inside.
    expect(painted('"Smith, John",42\n')[0]).toEqual(['"Smith, John"']);
    expect(painted('"Smith, John",42\n')[1]).toEqual(['42']);
  });

  it('skips an empty field rather than marking a zero-width range', () => {
    const plan = painted('a,,c\n');
    expect(plan[0]).toEqual(['a']);
    expect(plan[1]).toEqual([]);
    expect(plan[2]).toEqual(['c']);
  });

  it('paints only the window it was given', () => {
    const text = 'r0\nr1\nr2\nr3\n';
    expect(painted(text, 1, 2)[0]).toEqual(['r1', 'r2']);
  });

  it('clamps a window that hangs off either end', () => {
    const text = 'r0\nr1\n';
    expect(painted(text, -50, 5000)[0]).toEqual(['r0', 'r1']);
    expect(painted(text, 9, 20)[0]).toEqual([]);
  });

  it('follows a record that spans lines', () => {
    const text = 'a,b\n"two\nlines",z\n';
    expect(painted(text)[0]).toEqual(['a', '"two\nlines"']);
  });

  it('has nothing to paint in an empty file', () => {
    expect(painted('').every((bucket) => bucket.length === 0)).toBe(true);
  });
});

describe('naming a column', () => {
  it('counts the way a spreadsheet does', () => {
    expect(columnLabel(0)).toBe('A');
    expect(columnLabel(25)).toBe('Z');
    // Bijective base 26: after Z comes AA, not BA.
    expect(columnLabel(26)).toBe('AA');
    expect(columnLabel(27)).toBe('AB');
    expect(columnLabel(51)).toBe('AZ');
    expect(columnLabel(52)).toBe('BA');
    expect(columnLabel(701)).toBe('ZZ');
    expect(columnLabel(702)).toBe('AAA');
  });

  it('never returns nothing', () => {
    expect(columnLabel(-3)).toBe('A');
  });
});
