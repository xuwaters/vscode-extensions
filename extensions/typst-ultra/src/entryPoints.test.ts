import { describe, expect, it } from 'vitest';
import {
  documentSearchGlob,
  rankEntryCandidates,
  walkForDocuments,
  type DirectoryTree,
  type WalkBudget,
} from './entryPoints.js';

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

/** A directory tree in memory, and a log of what the walk actually read. */
function tree(
  layout: Record<string, [string, boolean][]>,
  reads: string[] = [],
): DirectoryTree<string> {
  return {
    async read(directory) {
      reads.push(directory);
      const entries = layout[directory];
      if (!entries) throw new Error(`cannot read ${directory}`);
      return entries;
    },
    join: (directory, name) => `${directory}/${name}`,
    id: (directory) => directory,
  };
}

const file = (name: string): [string, boolean] => [name, false];
const dir = (name: string): [string, boolean] => [name, true];

describe('walkForDocuments', () => {
  it('finds the documents beside the reader', async () => {
    const layout = { '/w': [file('main.typ'), file('notes.md'), file('lib.typc')] };
    expect(await walkForDocuments(['/w'], tree(layout))).toEqual([
      '/w/main.typ',
      '/w/lib.typc',
    ]);
  });

  it('descends as far as the budget allows and no further', async () => {
    const layout = {
      '/w': [dir('a')],
      '/w/a': [dir('b'), file('one.typ')],
      '/w/a/b': [dir('c'), file('two.typ')],
      '/w/a/b/c': [file('three.typ')],
    };
    expect(await walkForDocuments(['/w'], tree(layout))).toEqual([
      '/w/a/one.typ',
      '/w/a/b/two.typ',
    ]);
  });

  it('walks past the directories a document is never in', async () => {
    const reads: string[] = [];
    const layout = {
      '/w': [dir('node_modules'), dir('.git'), dir('target'), dir('chapters')],
      '/w/chapters': [file('one.typ')],
    };
    expect(await walkForDocuments(['/w'], tree(layout, reads))).toEqual([
      '/w/chapters/one.typ',
    ]);
    expect(reads).toEqual(['/w', '/w/chapters']);
  });

  it('stops at the directory budget, closest root first', async () => {
    const reads: string[] = [];
    const layout = {
      '/w/src': [file('chapter.typ')],
      '/w': [file('main.typ')],
    };
    const budget: WalkBudget = { depth: 2, dirs: 1, files: 48 };
    expect(await walkForDocuments(['/w/src', '/w'], tree(layout, reads), budget)).toEqual([
      '/w/src/chapter.typ',
    ]);
    expect(reads).toEqual(['/w/src']);
  });

  it('stops once it has enough to show', async () => {
    const layout = {
      '/w': [dir('a'), file('one.typ'), file('two.typ')],
      '/w/a': [file('three.typ')],
    };
    const budget: WalkBudget = { depth: 2, dirs: 24, files: 2 };
    expect(await walkForDocuments(['/w'], tree(layout), budget)).toEqual([
      '/w/one.typ',
      '/w/two.typ',
    ]);
  });

  it('carries on past a directory it cannot read', async () => {
    const layout = {
      '/w': [dir('locked'), dir('chapters')],
      '/w/chapters': [file('one.typ')],
    };
    expect(await walkForDocuments(['/w'], tree(layout))).toEqual(['/w/chapters/one.typ']);
  });

  it('reads a directory once when the roots overlap', async () => {
    const reads: string[] = [];
    const layout = {
      '/w': [dir('src'), file('main.typ')],
      '/w/src': [file('chapter.typ')],
    };
    const found = await walkForDocuments(['/w/src', '/w'], tree(layout, reads));
    expect(found).toEqual(['/w/src/chapter.typ', '/w/main.typ']);
    expect(reads).toEqual(['/w/src', '/w']);
  });

  it('has nothing to offer for no roots', async () => {
    expect(await walkForDocuments([], tree({}))).toEqual([]);
  });
});

describe('documentSearchGlob', () => {
  it('searches for what was typed, anywhere in the name', () => {
    expect(documentSearchGlob('chapter')).toBe('**/*chapter*.{typ,typc}');
  });

  it('is every document when nothing has been typed', () => {
    expect(documentSearchGlob('   ')).toBe('**/*.{typ,typc}');
  });

  it('drops an extension the pattern supplies itself', () => {
    expect(documentSearchGlob('main.typ')).toBe('**/*main*.{typ,typc}');
    expect(documentSearchGlob('main.ty')).toBe('**/*main*.{typ,typc}');
    expect(documentSearchGlob('main.typc')).toBe('**/*main*.{typ,typc}');
  });

  it('keeps a dot that is part of the name', () => {
    expect(documentSearchGlob('notes.draft')).toBe('**/*notes.draft*.{typ,typc}');
  });

  it('searches on the last segment of a typed path, since `*` stops at a slash', () => {
    expect(documentSearchGlob('chapters/one')).toBe('**/*one*.{typ,typc}');
    expect(documentSearchGlob('chapters\\one')).toBe('**/*one*.{typ,typc}');
  });

  it('takes glob characters in a name as the typos they are', () => {
    expect(documentSearchGlob('ma*in{')).toBe('**/*main*.{typ,typc}');
  });
});
