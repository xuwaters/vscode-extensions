import { describe, expect, it } from 'vitest';
import { formatZoomPercent, parseZoomPercent } from './zoom.js';

describe('reading a typed zoom', () => {
  it('reads a plain percentage, with or without the sign', () => {
    expect(parseZoomPercent('150')).toBe(150);
    expect(parseZoomPercent('150%')).toBe(150);
    expect(parseZoomPercent('  150 % ')).toBe(150);
    expect(parseZoomPercent('87.5%')).toBe(87.5);
    expect(parseZoomPercent('.5')).toBe(0.5);
  });

  it('reads a factor', () => {
    expect(parseZoomPercent('1.5x')).toBe(150);
    expect(parseZoomPercent('2X')).toBe(200);
  });

  // Nothing here should move the page: an unreadable value leaves the zoom as
  // it was and the box is rewritten from the real scale.
  it('rejects what is not a zoom', () => {
    expect(parseZoomPercent('')).toBeNull();
    expect(parseZoomPercent('%')).toBeNull();
    expect(parseZoomPercent('fit')).toBeNull();
    expect(parseZoomPercent('1e3')).toBeNull();
    expect(parseZoomPercent('12,5')).toBeNull();
    expect(parseZoomPercent('150%%')).toBeNull();
    expect(parseZoomPercent('-150')).toBeNull();
    expect(parseZoomPercent('0')).toBeNull();
    expect(parseZoomPercent('0x')).toBeNull();
  });

  // The box shows what it parses, so a round trip has to survive.
  it('round-trips what it prints', () => {
    for (const scale of [0.1, 0.5, 1, 1.25, 3, 20]) {
      expect(parseZoomPercent(formatZoomPercent(scale))).toBe(Math.round(scale * 100));
    }
  });
});
