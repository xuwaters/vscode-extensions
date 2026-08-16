import assert from 'node:assert/strict';
import { mkdirSync, writeFileSync } from 'node:fs';
import { describe, it } from 'node:test';

import { toNameList } from '../lib/repo.mjs';
import { makeRepo } from './helpers.mjs';

describe('repo', () => {
  it('lists extension directories in sorted order', () => {
    const repo = makeRepo({ zebra: {}, alpha: {}, middle: {} });
    assert.deepEqual(repo.listExtensions(), ['alpha', 'middle', 'zebra']);
  });

  it('ignores directories without a package.json', () => {
    const repo = makeRepo({ real: {} });
    mkdirSync(repo.path('not-an-extension'), { recursive: true });
    writeFileSync(repo.path('not-an-extension', 'README.md'), 'hi');
    assert.deepEqual(repo.listExtensions(), ['real']);
  });

  it('resolves an empty filter to every extension', () => {
    const repo = makeRepo({ a: {}, b: {} });
    assert.deepEqual(repo.resolveTargets([]), ['a', 'b']);
  });

  it('keeps repo order when filtering', () => {
    const repo = makeRepo({ a: {}, b: {}, c: {} });
    assert.deepEqual(repo.resolveTargets(['c', 'a']), ['a', 'c']);
  });

  it('names the unknown extension rather than silently skipping it', () => {
    const repo = makeRepo({ a: {} });
    assert.throws(() => repo.resolveTargets(['a', 'nope']), /Unknown extension: nope/);
  });

  it('reports a missing extensions directory as a usage error', () => {
    const repo = makeRepo({});
    assert.throws(() => repo.listExtensions(), /No extensions directory/);
  });
});

describe('toNameList', () => {
  it('splits and trims repeated, comma-separated values', () => {
    assert.deepEqual(toNameList(['a,b', ' c ']), ['a', 'b', 'c']);
    assert.deepEqual(toNameList('solo'), ['solo']);
  });

  it('treats absent and boolean values as no filter', () => {
    assert.deepEqual(toNameList(undefined), []);
    assert.deepEqual(toNameList(true), []);
    assert.deepEqual(toNameList(['', ' ']), []);
  });
});
