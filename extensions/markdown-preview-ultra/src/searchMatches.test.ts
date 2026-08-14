import { describe, expect, it } from 'vitest';
import { firstSearchMatch } from './searchMatches';

/** A Copy All dump as the search view formats one, matches indented by two. */
const RESULTS = [
  'docs/guide.md',
  '  12,5: the line that matched',
  '  40,1: another one',
  '',
  'notes/todo.md',
  '  3,11: something else entirely',
].join('\n');

describe('firstSearchMatch', () => {
  it('places a match, counting from zero', () => {
    expect(firstSearchMatch(RESULTS, '/repo/docs/guide.md')).toEqual({
      line: 11,
      column: 4,
      text: 'the line that matched',
    });
  });

  it('keeps each file to its own matches', () => {
    expect(firstSearchMatch(RESULTS, '/repo/notes/todo.md')).toEqual({
      line: 2,
      column: 10,
      text: 'something else entirely',
    });
  });

  it('finds nothing for a file the search did not reach', () => {
    expect(firstSearchMatch(RESULTS, '/repo/docs/other.md')).toBeUndefined();
  });

  it('finds nothing in an empty result set', () => {
    expect(firstSearchMatch('', '/repo/docs/guide.md')).toBeUndefined();
  });

  it('does not match a path that merely ends in the same characters', () => {
    // `…/myguide.md` ends with `guide.md`, but names a different file.
    expect(firstSearchMatch(RESULTS, '/repo/docs/myguide.md')).toBeUndefined();
  });

  it('matches a label that is the whole path', () => {
    const results = ['guide.md', '  1,1: at the top'].join('\n');
    expect(firstSearchMatch(results, 'guide.md')?.line).toBe(0);
  });

  it('matches a tildified label, for a file outside the workspace', () => {
    const results = ['~/notes/guide.md', '  7,2: away from home'].join('\n');
    expect(firstSearchMatch(results, '/Users/reader/notes/guide.md')).toEqual({
      line: 6,
      column: 1,
      text: 'away from home',
    });
  });

  it('matches a Windows path against a posix label', () => {
    const results = ['docs/guide.md', '  2,1: over there'].join('\n');
    expect(firstSearchMatch(results, 'C:\\repo\\docs\\guide.md')?.line).toBe(1);
  });

  it('takes the first match of a file, not of the dump', () => {
    const results = [
      'notes/todo.md',
      '  3,1: first file',
      '',
      'docs/guide.md',
      '  9,1: second file',
    ].join('\n');
    expect(firstSearchMatch(results, '/repo/docs/guide.md')?.line).toBe(8);
  });

  it('skips the continuation lines of a match that spans lines', () => {
    const results = [
      'docs/guide.md',
      '  12,5: opens here and',
      '  13:   carries on here',
      '',
      'notes/todo.md',
      '  1,1: elsewhere',
    ].join('\n');
    expect(firstSearchMatch(results, '/repo/notes/todo.md')?.line).toBe(0);
  });

  it('keeps the leading whitespace of an indented source line', () => {
    const results = ['docs/guide.md', '  4,7: 	- an indented item'].join('\n');
    expect(firstSearchMatch(results, '/repo/docs/guide.md')?.text).toBe(
      '\t- an indented item',
    );
  });

  it('reads a dump with carriage returns', () => {
    const results = 'docs/guide.md\r\n  12,5: the line that matched\r\n';
    expect(firstSearchMatch(results, '/repo/docs/guide.md')?.line).toBe(11);
  });

  it('ignores a match before any file has been named', () => {
    expect(firstSearchMatch('  12,5: orphaned', '/repo/a.md')).toBeUndefined();
  });
});
