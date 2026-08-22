import * as fs from 'node:fs';
import * as path from 'node:path';
import { describe, expect, it } from 'vitest';
import { RAINBOW_COLORS } from './paint.js';

/**
 * The rainbow is only worth having if a reader can tell one column from the one
 * beside it at a glance. A spectral palette — red, orange, yellow, … — fails
 * exactly where it matters: neighbouring columns get neighbouring hues, and two
 * columns of numbers in salmon and apricot read as one.
 *
 * So the defaults hand out ten evenly spaced hues in *steps of three* round the
 * wheel, and alternate lightness on top, and this test is what keeps them that
 * way. The threshold is a distance in OKLab, where 1.0 spans black to white and
 * about 0.02 is the smallest difference an eye can find at all.
 */

interface ColorContribution {
  id: string;
  defaults: Record<string, string>;
}

// vitest runs from the package root.
const manifest = JSON.parse(
  fs.readFileSync(path.resolve(process.cwd(), 'package.json'), 'utf8'),
) as { contributes: { colors: ColorContribution[] } };
const columns = manifest.contributes.colors.filter((color) =>
  /^csvUltra\.column\d+$/.test(color.id),
);

/**
 * The theme each variant is judged against: VSCode's own default editor
 * backgrounds, which is what these colours are actually read on.
 */
const BACKGROUNDS: Record<string, string> = {
  dark: '#1F1F1F',
  light: '#FFFFFF',
  highContrast: '#000000',
  highContrastLight: '#FFFFFF',
};

/** sRGB hex to linear-light channels. */
function linear(hex: string): [number, number, number] {
  const n = Number.parseInt(hex.slice(1), 16);
  const channels = [(n >> 16) & 255, (n >> 8) & 255, n & 255].map((value) => {
    const u = value / 255;
    return u <= 0.04045 ? u / 12.92 : ((u + 0.055) / 1.055) ** 2.4;
  });
  return channels as [number, number, number];
}

/** Linear sRGB to OKLab, whose distances match what an eye reports. */
function oklab([r, g, b]: [number, number, number]): [number, number, number] {
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
  return [
    0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
    1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
    0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s,
  ];
}

function distance(one: string, other: string): number {
  const a = oklab(linear(one));
  const b = oklab(linear(other));
  return Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);
}

/** WCAG relative-luminance contrast, the ratio the accessibility rules use. */
function contrast(one: string, other: string): number {
  const luminance = (hex: string) => {
    const [r, g, b] = linear(hex);
    return 0.2126 * r + 0.7152 * g + 0.0722 * b;
  };
  const [high, low] = [luminance(one), luminance(other)].sort((a, b) => b - a);
  return (high! + 0.05) / (low! + 0.05);
}

describe('the rainbow palette', () => {
  it('registers one colour per step of the cycle, for every theme kind', () => {
    expect(columns.map((color) => color.id)).toEqual(
      Array.from({ length: RAINBOW_COLORS }, (_, index) => `csvUltra.column${index + 1}`),
    );
    for (const color of columns) {
      expect(Object.keys(color.defaults).sort()).toEqual(Object.keys(BACKGROUNDS).sort());
      for (const value of Object.values(color.defaults)) {
        expect(value).toMatch(/^#[0-9A-Fa-f]{6}$/);
      }
    }
  });

  for (const theme of Object.keys(BACKGROUNDS)) {
    describe(theme, () => {
      const values = columns.map((color) => color.defaults[theme]!);

      it('keeps neighbouring columns far apart', () => {
        // Wraps: column 10 sits next to column 11, which is column 1 again.
        for (let index = 0; index < values.length; index += 1) {
          const here = values[index]!;
          const next = values[(index + 1) % values.length]!;
          expect([`${here} vs ${next}`, distance(here, next) > 0.15]).toEqual([
            `${here} vs ${next}`,
            true,
          ]);
        }
      });

      it('keeps every pair apart, not just the neighbours', () => {
        // Two columns a screen apart still get compared; the bar is lower than
        // for neighbours, but a duplicate is a duplicate.
        for (let i = 0; i < values.length; i += 1) {
          for (let j = i + 1; j < values.length; j += 1) {
            expect(distance(values[i]!, values[j]!)).toBeGreaterThan(0.08);
          }
        }
      });

      it('stays readable on the background it is read on', () => {
        for (const value of values) {
          expect(contrast(value, BACKGROUNDS[theme]!)).toBeGreaterThan(4.5);
        }
      });
    });
  }
});
