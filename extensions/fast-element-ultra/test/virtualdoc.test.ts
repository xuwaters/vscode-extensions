/**
 * P1-04: the virtual-document substitution. Length preservation is the whole
 * coordinate model — `sourceOffset = templateStart + documentOffset`, with no
 * mapping table — so it is tested as a property over generated templates, not
 * as examples.
 */

import { describe, expect, it } from 'vitest';

import { substitute } from '../tsplugin/extract.js';
import type { PlaceholderFact } from '../tsplugin/protocol.js';

/** Deterministic pseudo-random generator: tests must not flake. */
function mulberry32(seed: number): () => number {
  let a = seed;
  return () => {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function generate(seed: number): { text: string; placeholders: PlaceholderFact[] } {
  const rand = mulberry32(seed);
  const literals = ['<div class="', '">', '<span>', '</span>', ' title="a · ⌘K" ', '<path d="M0 0"/>', 'text ✕ more'];
  let text = '';
  const placeholders: PlaceholderFact[] = [];
  let index = 0;
  const parts = 3 + Math.floor(rand() * 10);
  for (let i = 0; i < parts; i++) {
    text += literals[Math.floor(rand() * literals.length)];
    if (rand() < 0.6) {
      const exprLength = 2 + Math.floor(rand() * 40);
      const start = text.length;
      // The region covers `${…}` in the source; content is irrelevant here.
      text += '$' + '{'.padEnd(exprLength - 2, 'x') + '}';
      placeholders.push({ index: index++, start, end: text.length });
    }
  }
  return { text, placeholders };
}

describe('the substitution', () => {
  it('preserves length and never touches literal parts, over 200 generated templates', () => {
    for (let seed = 1; seed <= 200; seed++) {
      const { text, placeholders } = generate(seed);
      const substituted = substitute(text, placeholders);
      expect(substituted.length, `seed ${seed}`).toBe(text.length);
      // Every offset outside a placeholder maps to the identical character.
      let cursor = 0;
      for (const placeholder of placeholders) {
        expect(substituted.slice(cursor, placeholder.start)).toBe(
          text.slice(cursor, placeholder.start),
        );
        cursor = placeholder.end;
      }
      expect(substituted.slice(cursor)).toBe(text.slice(cursor));
      // Placeholder runs are underscores with at most a base-36 index inside.
      for (const placeholder of placeholders) {
        const run = substituted.slice(placeholder.start, placeholder.end);
        expect(run, `seed ${seed}`).toMatch(/^_+[0-9a-z]*_+$|^_+$/);
      }
    }
  });

  it('writes a recoverable index into runs long enough to hold one', () => {
    const text = 'a${xxxxxxxx}b${yyyyyyyy}c';
    const placeholders: PlaceholderFact[] = [
      { index: 0, start: 1, end: 12 },
      { index: 1, start: 13, end: 24 },
    ];
    const substituted = substitute(text, placeholders);
    expect(substituted).toBe('a__0________b__1________c');
  });

  it('fits a one-character index into the smallest real expression', () => {
    // Index 35 is 'z' in base 36; a 4-character run still holds it.
    expect(substitute('${x}', [{ index: 35, start: 0, end: 4 }])).toBe('__z_');
    // Too small for the index: plain underscores.
    expect(substitute('${}', [{ index: 0, start: 0, end: 3 }])).toBe('___');
  });
});
