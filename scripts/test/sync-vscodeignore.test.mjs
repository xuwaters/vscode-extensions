import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { describe, it } from 'node:test';

import { TEMPLATE_PATH, syncVscodeignoreCommand } from '../commands/sync-vscodeignore.mjs';
import { makeRepo, makeWriter, readExtensionFile } from './helpers.mjs';

const TEMPLATE = readFileSync(TEMPLATE_PATH, 'utf8');

/**
 * @param {import('../lib/repo.mjs').Repo} repo
 * @param {Record<string, boolean | string | string[] | undefined>} values
 * @returns {{ code: number, out: string }}
 */
function sync(repo, values) {
  const writer = makeWriter();
  const code = Number(
    syncVscodeignoreCommand.run({ values, positionals: [], repo, write: writer.write }) ?? 0,
  );
  return { code, out: writer.text() };
}

describe('sync-vscodeignore', () => {
  it('overwrites a stale copy and creates a missing one', () => {
    const repo = makeRepo({ stale: { vscodeignore: 'src/\n' }, missing: {} });
    const { code, out } = sync(repo, {});

    assert.equal(code, 0);
    assert.equal(readExtensionFile(repo, 'stale', '.vscodeignore'), TEMPLATE);
    assert.equal(readExtensionFile(repo, 'missing', '.vscodeignore'), TEMPLATE);
    assert.match(out, /stale\s+updated/);
    assert.match(out, /missing\s+created/);
    assert.match(out, /2 extensions changed/);
  });

  it('leaves a matching copy alone', () => {
    const repo = makeRepo({ current: { vscodeignore: TEMPLATE } });
    const { out } = sync(repo, {});
    assert.match(out, /current\s+up to date/);
    assert.match(out, /0 extensions changed/);
  });

  it('syncs only the filtered extensions', () => {
    const repo = makeRepo({ a: { vscodeignore: 'old\n' }, b: { vscodeignore: 'old\n' } });
    sync(repo, { filter: ['a'] });
    assert.equal(readExtensionFile(repo, 'a', '.vscodeignore'), TEMPLATE);
    assert.equal(readExtensionFile(repo, 'b', '.vscodeignore'), 'old\n');
  });

  it('writes nothing on --dry-run', () => {
    const repo = makeRepo({ a: { vscodeignore: 'old\n' } });
    const { out } = sync(repo, { 'dry-run': true });
    assert.equal(readExtensionFile(repo, 'a', '.vscodeignore'), 'old\n');
    assert.match(out, /would be updated/);
  });

  it('fails --check on drift, pointing at the fix', () => {
    const repo = makeRepo({ a: { vscodeignore: 'old\n' } });
    const { code, out } = sync(repo, { check: true });
    assert.equal(code, 1);
    assert.equal(readExtensionFile(repo, 'a', '.vscodeignore'), 'old\n');
    assert.match(out, /Run `pnpm sync-vscodeignore`/);
  });

  it('passes --check when everything matches', () => {
    const repo = makeRepo({ a: { vscodeignore: TEMPLATE } });
    assert.equal(sync(repo, { check: true }).code, 0);
  });
});

describe('the shipped template', () => {
  it('excludes sources and build output, and keeps what extensions contribute', () => {
    const lines = TEMPLATE.split('\n')
      .map((line) => line.trim())
      .filter((line) => line && !line.startsWith('#'));

    for (const entry of ['src/', 'test/', 'examples/', 'webview/', 'tsconfig.json', 'temp']) {
      assert.ok(lines.includes(entry), `template should exclude ${entry}`);
    }
    for (const kept of ['dist', 'syntaxes', 'snippets', 'wasm', 'package.json', 'README.md']) {
      assert.ok(!lines.includes(kept), `template must not exclude ${kept}`);
    }
  });
});
