import { describe, expect, it } from 'vitest';
import { formatZoomPercent, parseZoomPercent } from './zoom.js';

describe('reading a zoom the reader typed', () => {
  it('accepts the ways a percentage gets written', () => {
    expect(parseZoomPercent('150')).toBe(150);
    expect(parseZoomPercent('150%')).toBe(150);
    expect(parseZoomPercent('  150 % ')).toBe(150);
    expect(parseZoomPercent('87.5%')).toBe(87.5);
    expect(parseZoomPercent('.5')).toBe(0.5);
  });

  it('reads a factor as the percentage it means', () => {
    expect(parseZoomPercent('1.5x')).toBe(150);
    expect(parseZoomPercent('2X')).toBe(200);
  });

  it('rejects anything it cannot read, so the current zoom is left alone', () => {
    expect(parseZoomPercent('')).toBeNull();
    expect(parseZoomPercent('abc')).toBeNull();
    expect(parseZoomPercent('1.5.2')).toBeNull();
    expect(parseZoomPercent('-50%')).toBeNull();
    expect(parseZoomPercent('0')).toBeNull();
    expect(parseZoomPercent('50 percent')).toBeNull();
  });
});

describe('writing it back', () => {
  it('rounds to whole percent', () => {
    expect(formatZoomPercent(1)).toBe('100%');
    expect(formatZoomPercent(0.8751)).toBe('88%');
  });
});
