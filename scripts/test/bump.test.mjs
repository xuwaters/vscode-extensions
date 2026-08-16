import assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';
import { describe, it } from 'node:test';

import { UsageError } from '../lib/cli.mjs';
import { bumpCommand } from '../commands/bump.mjs';
import { makeRepo, makeWriter, readExtensionFile } from './helpers.mjs';

/**
 * @param {import('../lib/repo.mjs').Repo} repo
 * @param {Record<string, boolean | string | string[] | undefined>} values
 * @returns {{ code: number, out: string }}
 */
function bump(repo, values) {
  const writer = makeWriter();
  const code = Number(bumpCommand.run({ values, positionals: [], repo, write: writer.write }) ?? 0);
  return { code, out: writer.text() };
}

/**
 * @param {import('../lib/repo.mjs').Repo} repo
 * @param {string} name
 * @returns {string}
 */
function versionOf(repo, name) {
  return JSON.parse(readExtensionFile(repo, name, 'package.json')).version;
}

describe('bump', () => {
  it('increments the patch by default', () => {
    const repo = makeRepo({ a: { version: '1.2.3' }, b: { version: '0.0.9' } });
    bump(repo, {});
    assert.equal(versionOf(repo, 'a'), '1.2.4');
    assert.equal(versionOf(repo, 'b'), '0.0.10');
  });

  it('honours --minor and --major', () => {
    const repo = makeRepo({ a: { version: '1.2.3' } });
    bump(repo, { minor: true });
    assert.equal(versionOf(repo, 'a'), '1.3.0');
    bump(repo, { major: true });
    assert.equal(versionOf(repo, 'a'), '2.0.0');
  });

  it('bumps only the filtered extensions', () => {
    const repo = makeRepo({ a: { version: '1.0.0' }, b: { version: '1.0.0' } });
    bump(repo, { filter: ['a'] });
    assert.equal(versionOf(repo, 'a'), '1.0.1');
    assert.equal(versionOf(repo, 'b'), '1.0.0');
  });

  it('sets an exact version with --set', () => {
    const repo = makeRepo({ a: { version: '1.2.3' } });
    bump(repo, { set: '9.0.0' });
    assert.equal(versionOf(repo, 'a'), '9.0.0');
  });

  it('writes nothing on --dry-run but reports what it would do', () => {
    const repo = makeRepo({ a: { version: '1.2.3' } });
    const { out } = bump(repo, { 'dry-run': true });
    assert.equal(versionOf(repo, 'a'), '1.2.3');
    assert.match(out, /\[dry-run\]/);
    assert.match(out, /1\.2\.3 -> 1\.2\.4/);
    assert.match(out, /1 extension would change/);
  });

  it('counts a no-op --set as unchanged', () => {
    const repo = makeRepo({ a: { version: '1.2.3' } });
    const { out } = bump(repo, { set: '1.2.3' });
    assert.match(out, /1\.2\.3 \(unchanged\)/);
    assert.match(out, /0 extensions changed/);
  });

  it('preserves formatting of the file it edits', () => {
    const repo = makeRepo({ a: { version: '1.0.0' } });
    const path = repo.path('a', 'package.json');
    writeFileSync(path, '{\n\t"name": "a",\n\t"version": "1.0.0"\n}');
    bump(repo, {});
    const raw = readFileSync(path, 'utf8');
    assert.match(raw, /\n\t"version": "1\.0\.1"/);
    assert.ok(!raw.endsWith('\n'), 'should not gain a trailing newline');
  });

  it('rejects contradictory and malformed input', () => {
    const repo = makeRepo({ a: { version: '1.0.0' } });
    assert.throws(() => bump(repo, { minor: true, major: true }), UsageError);
    assert.throws(() => bump(repo, { set: 'v1' }), UsageError);
    assert.throws(() => bump(repo, { filter: ['ghost'] }), UsageError);
    assert.equal(versionOf(repo, 'a'), '1.0.0');
  });

  it('refuses a package.json with no version field', () => {
    const repo = makeRepo({ a: {} });
    writeFileSync(repo.path('a', 'package.json'), '{"name":"a"}');
    assert.throws(() => bump(repo, {}), /has no string 'version' field/);
  });
});
