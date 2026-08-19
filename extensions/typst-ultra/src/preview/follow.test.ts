import { describe, expect, it } from 'vitest';
import { decideFollow, type FollowInputs } from './follow.js';

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
