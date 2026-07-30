import { describe, expect, it } from 'vitest';
import { DEFAULT_DESCEND, DEFAULT_NAMES, DEFAULT_PRUNE, nameList, resolveConfig } from './settings.js';

describe('nameList', () => {
  it('falls back when the value is missing or malformed', () => {
    expect(nameList(undefined, DEFAULT_NAMES)).toEqual(['.cc-writes']);
    expect(nameList('.cc-writes', DEFAULT_NAMES)).toEqual(['.cc-writes']);
  });

  it('leaves an explicitly empty list empty', () => {
    expect(nameList([], DEFAULT_PRUNE)).toEqual([]);
  });

  it('trims, deduplicates and drops what cannot be a base name', () => {
    expect(nameList([' .cc-writes ', '.cc-writes', 'a/b', '', '.', '..', '.git', 7], DEFAULT_NAMES)).toEqual([
      '.cc-writes',
    ]);
  });
});

describe('resolveConfig', () => {
  it('fills in the defaults', () => {
    const cfg = resolveConfig({}, false);
    expect(cfg).toEqual({
      names: DEFAULT_NAMES,
      descend: DEFAULT_DESCEND,
      prune: DEFAULT_PRUNE,
      dryRun: false,
      noIgnore: false,
    });
  });

  it('reads the settings it is given', () => {
    const cfg = resolveConfig(
      { names: ['.cache'], descend: ['.x'], prune: [], respectGitignore: false },
      true,
    );
    expect(cfg).toEqual({
      names: ['.cache'],
      descend: ['.x'],
      prune: [],
      dryRun: true,
      noIgnore: true,
    });
  });
});
