import * as path from 'path';
import { describe, expect, it } from 'vitest';
import { fontCandidates, resolveBundledFonts } from './bundledFonts.js';

const INSTALLED = path.join('/ext', 'weixu.wx-vsce-typst-ultra-0.3.2');
const COMPANION = path.join('/ext', 'weixu.wx-vsce-typst-ultra-fonts-0.3.1');

/** A probe that says yes to exactly the given directories. */
function only(...dirs: string[]): (dir: string) => boolean {
  const set = new Set(dirs.map((dir) => path.normalize(dir)));
  return (dir) => set.has(path.normalize(dir));
}

describe('where the bundled fonts are looked for', () => {
  it('prefers the companion extension', () => {
    const found = resolveBundledFonts(
      INSTALLED,
      COMPANION,
      only(
        path.join(COMPANION, 'assets', 'fonts'),
        path.join(INSTALLED, 'assets', 'fonts'),
      ),
    );

    expect(found).toEqual({
      path: path.join(COMPANION, 'assets', 'fonts'),
      source: 'companion',
    });
  });

  // The upgrade path: earlier releases shipped the fonts inside this
  // extension, and an install that skipped the companion still has them.
  it('falls back to fonts inside this extension', () => {
    const found = resolveBundledFonts(
      INSTALLED,
      undefined,
      only(path.join(INSTALLED, 'assets', 'fonts')),
    );

    expect(found?.source).toBe('in-place');
  });

  // `F5` from the checkout: the companion is a directory, not an extension.
  it('finds the sibling package in a checkout', () => {
    const checkout = path.join('/repo', 'extensions', 'typst-ultra');
    const found = resolveBundledFonts(
      checkout,
      undefined,
      only(path.join('/repo', 'extensions', 'typst-ultra-fonts', 'assets', 'fonts')),
    );

    expect(found?.source).toBe('sibling');
  });

  // `pnpm run clean` leaves the directory, so existence is not the test.
  it('skips a directory with no fonts in it', () => {
    expect(resolveBundledFonts(INSTALLED, COMPANION, () => false)).toBeUndefined();
  });

  it('offers no companion candidate when the companion is not installed', () => {
    expect(fontCandidates(INSTALLED).map((c) => c.source)).toEqual([
      'in-place',
      'sibling',
    ]);
  });
});
