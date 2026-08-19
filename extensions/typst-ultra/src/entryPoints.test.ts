import { describe, expect, it } from 'vitest';
import { rankEntryCandidates } from './entryPoints.js';

describe('rankEntryCandidates', () => {
  it('floats a conventional name to the top', () => {
    expect(rankEntryCandidates(['data.typ', 'template.typ', 'main.typ'])[0]).toBe(
      'main.typ',
    );
  });

  it('prefers the shallower of two conventional names', () => {
    expect(rankEntryCandidates(['src/main.typ', 'paper.typ'])[0]).toBe('paper.typ');
  });

  it('prefers a conventional name deeper down to an unconventional one above it', () => {
    expect(rankEntryCandidates(['lib.typ', 'doc/thesis.typ'])[0]).toBe(
      'doc/thesis.typ',
    );
  });

  it('puts shallow files first among equals', () => {
    expect(rankEntryCandidates(['chapters/one.typ', 'appendix.typ'])).toEqual([
      'appendix.typ',
      'chapters/one.typ',
    ]);
  });

  it('breaks ties alphabetically, so the same list twice is the same list', () => {
    expect(rankEntryCandidates(['b.typ', 'a.typ', 'c.typ'])).toEqual([
      'a.typ',
      'b.typ',
      'c.typ',
    ]);
  });

  it('reads backslashes as separators too', () => {
    expect(rankEntryCandidates(['deep\\nested\\main.typ', 'notes.typ'])[0]).toBe(
      'deep\\nested\\main.typ',
    );
  });

  it('accepts .typc as an entry point', () => {
    expect(rankEntryCandidates(['helpers.typ', 'main.typc'])[0]).toBe('main.typc');
  });

  it('does not mutate its argument', () => {
    const input = ['z.typ', 'main.typ'];
    rankEntryCandidates(input);
    expect(input).toEqual(['z.typ', 'main.typ']);
  });

  it('is empty for no candidates', () => {
    expect(rankEntryCandidates([])).toEqual([]);
  });
});
