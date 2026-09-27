import assert from 'node:assert/strict';
import { writeFileSync } from 'node:fs';
import { describe, it } from 'node:test';

import { UsageError } from '../lib/cli.mjs';
import { createPublishCommand } from '../commands/publish.mjs';
import { makeRepo, makeWriter } from './helpers.mjs';

/**
 * @typedef {{ command: string, args: string[], cwd: string }} Call
 */

/**
 * Run `publish` with an `exec` that records each call and answers with the
 * next exit code from `codes` (0 once they run out).
 *
 * @param {import('../lib/repo.mjs').Repo} repo
 * @param {Record<string, boolean | string | string[] | undefined>} values
 * @param {number[]} [codes]
 * @returns {Promise<{ code: number, out: string, calls: Call[] }>}
 */
async function publish(repo, values, codes = []) {
  /** @type {Call[]} */
  const calls = [];
  const command = createPublishCommand(async (command, args, { cwd }) => {
    calls.push({ command, args, cwd });
    return codes.shift() ?? 0;
  });
  const writer = makeWriter();
  const code = Number((await command.run({ values, positionals: [], repo, write: writer.write })) ?? 0);
  return { code, out: writer.text(), calls };
}

/**
 * @param {import('../lib/repo.mjs').Repo} repo
 * @param {string} name
 * @param {string} version
 * @returns {void}
 */
function writeVsix(repo, name, version) {
  writeFileSync(repo.path(name, `${name}-${version}.vsix`), '');
}

describe('publish', () => {
  it('publishes the .vsix for the current version of every extension', async () => {
    const repo = makeRepo({ a: { version: '1.2.3' }, b: { version: '0.1.0' } });
    writeVsix(repo, 'a', '1.2.3');
    writeVsix(repo, 'b', '0.1.0');

    const { code, calls, out } = await publish(repo, {});
    assert.equal(code, 0);
    assert.deepEqual(calls, [
      {
        command: 'pnpm',
        args: ['exec', 'vsce', 'publish', '--packagePath', 'a-1.2.3.vsix', '--skip-duplicate'],
        cwd: repo.path('a'),
      },
      {
        command: 'pnpm',
        args: ['exec', 'vsce', 'publish', '--packagePath', 'b-0.1.0.vsix', '--skip-duplicate'],
        cwd: repo.path('b'),
      },
    ]);
    assert.match(out, /2 extensions of 2/);
  });

  it('publishes only the filtered extensions', async () => {
    const repo = makeRepo({ a: {}, b: {} });
    writeVsix(repo, 'b', '1.0.0');
    const { calls } = await publish(repo, { filter: ['b'] });
    assert.deepEqual(calls.map((c) => c.cwd), [repo.path('b')]);
  });

  it('passes --azure-credential through to vsce', async () => {
    const repo = makeRepo({ a: {} });
    writeVsix(repo, 'a', '1.0.0');
    const { calls } = await publish(repo, { 'azure-credential': true });
    assert.ok(calls[0].args.includes('--azure-credential'));
  });

  it('uploads nothing when any .vsix is missing', async () => {
    const repo = makeRepo({ a: { version: '1.0.1' }, b: {} });
    writeVsix(repo, 'a', '1.0.0');
    writeVsix(repo, 'b', '1.0.0');

    /** @type {Call[]} */
    const calls = [];
    const command = createPublishCommand(async (command, args, { cwd }) => {
      calls.push({ command, args, cwd });
      return 0;
    });
    await assert.rejects(
      async () => command.run({ values: {}, positionals: [], repo, write: () => {} }),
      (error) => error instanceof UsageError && /a-1\.0\.1\.vsix/.test(error.message),
    );
    assert.deepEqual(calls, []);
  });

  it('builds every extension before publishing any with --build', async () => {
    const repo = makeRepo({ a: {}, b: {} });
    writeVsix(repo, 'a', '1.0.0');
    writeVsix(repo, 'b', '1.0.0');

    const { calls } = await publish(repo, { build: true });
    assert.deepEqual(
      calls.map((c) => [c.cwd, c.args.slice(0, 4).join(' ')]),
      [
        [repo.root, '--filter a run package'],
        [repo.root, '--filter b run package'],
        [repo.path('a'), 'exec vsce publish --packagePath'],
        [repo.path('b'), 'exec vsce publish --packagePath'],
      ],
    );
  });

  it('publishes nothing when a build fails', async () => {
    const repo = makeRepo({ a: {}, b: {} });
    const { code, calls, out } = await publish(repo, { build: true }, [0, 2]);
    assert.equal(code, 2);
    assert.equal(calls.length, 2);
    assert.match(out, /Building b failed \(exit 2\); nothing was published/);
  });

  it('stops at the first failed upload and reports what went out', async () => {
    const repo = makeRepo({ a: {}, b: {}, c: {} });
    for (const name of ['a', 'b', 'c']) writeVsix(repo, name, '1.0.0');

    const { code, calls, out } = await publish(repo, {}, [0, 1]);
    assert.equal(code, 1);
    assert.equal(calls.length, 2);
    assert.match(out, /Publishing b failed \(exit 1\)/);
    assert.match(out, /1 extension of 3/);
    assert.match(out, /\n {2}a {2}1\.0\.0/);
  });

  it('runs nothing on --dry-run but lists each step', async () => {
    const repo = makeRepo({ a: { version: '2.0.0' } });
    const { code, calls, out } = await publish(repo, { build: true, 'dry-run': true });
    assert.equal(code, 0);
    assert.deepEqual(calls, []);
    assert.match(out, /\[dry-run\] pnpm --filter a run package/);
    assert.match(out, /\[dry-run\] \(extensions\/a\) pnpm exec vsce publish --packagePath a-2\.0\.0\.vsix/);
  });

  it('still requires the .vsix on a dry run without --build', async () => {
    const repo = makeRepo({ a: {} });
    await assert.rejects(() => publish(repo, { 'dry-run': true }), UsageError);
  });
});
