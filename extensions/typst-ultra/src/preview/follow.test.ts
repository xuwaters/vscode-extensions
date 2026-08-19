import { describe, expect, it } from 'vitest';
import {
  chooseSubject,
  decideFollow,
  type FollowInputs,
  type RestoreInputs,
} from './follow.js';

const MAIN = 'file:///p/main.typ';
const DATA = 'file:///p/data.typ';
const OTHER = 'file:///p/other.typ';

function inputs(overrides: Partial<FollowInputs> = {}): FollowInputs {
  return {
    active: DATA,
    target: MAIN,
    entry: undefined,
    locked: false,
    ...overrides,
  };
}

describe('decideFollow', () => {
  it('follows the focus with no compile root set', () => {
    expect(decideFollow(inputs())).toBe('showActive');
  });

  it('stays when the focused file is already the subject', () => {
    expect(decideFollow(inputs({ active: MAIN }))).toBe('stay');
  });

  it('stays on the compile root when a part of the project is opened', () => {
    expect(decideFollow(inputs({ entry: MAIN }))).toBe('stay');
  });

  it('comes back to the compile root from a document that is not it', () => {
    expect(decideFollow(inputs({ entry: MAIN, target: OTHER }))).toBe('showEntry');
  });

  it('adopts the compile root when the panel has no subject yet', () => {
    expect(decideFollow(inputs({ entry: MAIN, target: undefined }))).toBe(
      'showEntry',
    );
  });

  it('adopts the compile root even while its own file is focused', () => {
    // Opening `data.typ` in a project is what used to blank the preview.
    expect(decideFollow(inputs({ entry: MAIN, target: DATA }))).toBe('showEntry');
  });

  it('takes the first subject when nothing is set at all', () => {
    expect(decideFollow(inputs({ target: undefined }))).toBe('showActive');
  });

  it('a lock outranks the compile root', () => {
    expect(decideFollow(inputs({ entry: MAIN, target: OTHER, locked: true }))).toBe(
      'stay',
    );
  });

  it('a lock outranks following the focus', () => {
    expect(decideFollow(inputs({ locked: true }))).toBe('stay');
  });
});

function restoring(overrides: Partial<RestoreInputs> = {}): RestoreInputs {
  return {
    entry: undefined,
    remembered: undefined,
    active: undefined,
    open: [],
    ...overrides,
  };
}

describe('chooseSubject', () => {
  it('comes back to what the panel was showing', () => {
    expect(chooseSubject(restoring({ remembered: MAIN }))).toBe(MAIN);
  });

  it('comes back to the compile root over anything else', () => {
    expect(
      chooseSubject(restoring({ entry: MAIN, remembered: OTHER, active: DATA })),
    ).toBe(MAIN);
  });

  it('keeps the panel on its own subject rather than the focused file', () => {
    // A pinned panel has to come back pinned to the file it was pinned to; an
    // unpinned one is put back on the focus by the next `decideFollow`.
    expect(chooseSubject(restoring({ remembered: MAIN, active: DATA }))).toBe(MAIN);
  });

  it('falls back to the focused file with nothing remembered', () => {
    expect(chooseSubject(restoring({ active: DATA, open: [OTHER] }))).toBe(DATA);
  });

  it('falls back to an open tab when the editors are not back yet', () => {
    // The case that used to render nothing: reloading with the focus inside the
    // panel leaves no active editor to read a subject from.
    expect(chooseSubject(restoring({ open: [OTHER, DATA] }))).toBe(OTHER);
  });

  it('has no answer when the window holds no typst file at all', () => {
    expect(chooseSubject(restoring())).toBeUndefined();
  });
});
