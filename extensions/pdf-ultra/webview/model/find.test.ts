import { describe, expect, it } from 'vitest';
import {
  findLabel,
  fold,
  joinItems,
  matchNear,
  matchesInPage,
  sliceMatch,
  stepMatch,
  type PageItem,
} from './find.js';

const run = (text: string, eol = false): PageItem => ({ text, eol });

describe('joining a page', () => {
  it('puts nothing between runs, because the document already drew the spaces', () => {
    const { text } = joinItems([run('in'), run('voice')]);
    expect(text).toBe('invoice');
  });

  it('puts a newline back where a line ended', () => {
    const { text } = joinItems([run('the', true), run('end')]);
    expect(text).toBe('the\nend');
  });

  it('maps each run to its own characters, and never to the newline', () => {
    const { spans } = joinItems([run('the', true), run('end')]);
    expect(spans).toEqual([
      { index: 0, start: 0, end: 3 },
      { index: 1, start: 4, end: 7 },
    ]);
  });
});

describe('folding', () => {
  it('is length-preserving, so offsets survive it', () => {
    const text = 'A B\tC\nD';
    expect(fold(text)).toHaveLength(text.length);
  });

  it('makes every kind of space the same space', () => {
    expect(fold('a b\tc')).toBe('a b c');
  });
});

describe('matching', () => {
  it('is case-insensitive', () => {
    expect(matchesInPage(1, 'Content-Security-Policy', 'security')).toEqual([
      { page: 1, start: 8, length: 8 },
    ]);
  });

  it('finds every occurrence, left to right', () => {
    expect(matchesInPage(3, 'aXaXa', 'a').map((m) => m.start)).toEqual([0, 2, 4]);
  });

  it('does not overlap matches', () => {
    expect(matchesInPage(1, 'aaaa', 'aa').map((m) => m.start)).toEqual([0, 2]);
  });

  it('matches a phrase across a line break, which reads as a space', () => {
    const { text } = joinItems([run('the', true), run('end')]);
    expect(matchesInPage(1, text, 'the end')).toEqual([{ page: 1, start: 0, length: 7 }]);
  });

  it('finds nothing for an empty needle rather than everything', () => {
    expect(matchesInPage(1, 'anything', '')).toEqual([]);
  });
});

describe('stepping through matches', () => {
  it('starts at the first going forwards and the last going back', () => {
    expect(stepMatch(3, -1, 1)).toBe(0);
    expect(stepMatch(3, -1, -1)).toBe(2);
  });

  it('wraps in both directions', () => {
    expect(stepMatch(3, 2, 1)).toBe(0);
    expect(stepMatch(3, 0, -1)).toBe(2);
  });

  it('has nowhere to go with no matches', () => {
    expect(stepMatch(0, -1, 1)).toBe(-1);
  });
});

describe('where a search starts', () => {
  const matches = [
    { page: 2, start: 0, length: 1 },
    { page: 7, start: 0, length: 1 },
  ];

  it('lands on the first match at or after the page being read', () => {
    expect(matchNear(matches, 1)).toBe(0);
    expect(matchNear(matches, 3)).toBe(1);
    expect(matchNear(matches, 7)).toBe(1);
  });

  it('wraps to the top when everything is behind the reader', () => {
    expect(matchNear(matches, 9)).toBe(0);
  });

  it('has nowhere to land with no matches', () => {
    expect(matchNear([], 1)).toBe(-1);
  });
});

describe('slicing a match across runs', () => {
  // pdf.js emits a run per change of font or position, so a phrase is rarely
  // one of them — "Content-Security-Policy" can easily be four.
  const { spans } = joinItems([run('Con'), run('tent-Sec'), run('urity')]);

  it('cuts a match into one slice per run it crosses', () => {
    expect(sliceMatch(spans, { page: 1, start: 0, length: 16 })).toEqual([
      { index: 0, start: 0, end: 3 },
      { index: 1, start: 0, end: 8 },
      { index: 2, start: 0, end: 5 },
    ]);
  });

  it('slices the middle of a single run', () => {
    expect(sliceMatch(spans, { page: 1, start: 4, length: 3 })).toEqual([
      { index: 1, start: 1, end: 4 },
    ]);
  });

  it('drops the newline between runs rather than slicing nothing out of it', () => {
    const joined = joinItems([run('the', true), run('end')]);
    expect(sliceMatch(joined.spans, { page: 1, start: 0, length: 7 })).toEqual([
      { index: 0, start: 0, end: 3 },
      { index: 1, start: 0, end: 3 },
    ]);
  });
});

describe('the readout', () => {
  it('says nothing at all when nothing was typed', () => {
    expect(findLabel('   ', 0, -1)).toBe('');
  });

  it('says so when a search found nothing', () => {
    expect(findLabel('xyz', 0, -1)).toBe('No results');
  });

  it('counts from one, the way a reader does', () => {
    expect(findLabel('a', 12, 0)).toBe('1 of 12');
    expect(findLabel('a', 12, 11)).toBe('12 of 12');
  });
});
