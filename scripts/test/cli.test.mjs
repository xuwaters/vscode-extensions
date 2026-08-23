import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import { UsageError, formatCommandHelp, formatOverviewHelp, parseCommandArgs, run } from '../lib/cli.mjs';
import { makeRepo, makeWriter } from './helpers.mjs';

/** @type {import('../lib/cli.mjs').Command<{ repo: import('../lib/repo.mjs').Repo }>} */
const echo = {
  name: 'echo',
  summary: 'Echo the flags it was given',
  options: {
    loud: { type: 'boolean', short: 'l', describe: 'Shout.' },
    text: { type: 'string', placeholder: '<words>', describe: 'What to say.' },
  },
  run({ values, write }) {
    write(String(values.text ?? ''));
    return values.loud === true ? 3 : 0;
  },
};

/**
 * @param {string[]} argv
 * @returns {Promise<{ code: number, out: string }>}
 */
async function invoke(argv) {
  const writer = makeWriter();
  const code = await run({
    binName: 'repo',
    commands: [echo],
    argv,
    context: { repo: makeRepo({ a: {} }) },
    write: writer.write,
  });
  return { code, out: writer.text() };
}

describe('cli dispatch', () => {
  it('runs the named command and returns its exit code', async () => {
    assert.deepEqual(await invoke(['echo', '--text', 'hi']), { code: 0, out: 'hi' });
    assert.equal((await invoke(['echo', '-l'])).code, 3);
  });

  it('lists the commands when asked for nothing in particular', async () => {
    for (const argv of [[], ['--help'], ['help']]) {
      const { code, out } = await invoke(argv);
      assert.equal(code, 0);
      assert.match(out, /Usage: repo <command>/);
      assert.match(out, /echo\s+Echo the flags/);
    }
  });

  it('shows a command help instead of running it', async () => {
    const { code, out } = await invoke(['echo', '--help']);
    assert.equal(code, 0);
    assert.match(out, /-l, --loud\s+Shout\./);
    assert.match(out, /--text <words>\s+What to say\./);
  });

  it('rejects an unknown command with the command list attached', async () => {
    await assert.rejects(invoke(['nope']), (error) => {
      assert.ok(error instanceof UsageError);
      assert.match(error.message, /Unknown command 'nope'/);
      assert.match(String(error.help), /Commands:/);
      return true;
    });
  });

  it('rejects an unknown flag as a usage error, not a crash', () => {
    assert.throws(() => parseCommandArgs(echo, ['--nope']), UsageError);
  });

  it('accepts a value for a string option only', () => {
    // parseArgs hands back a null-prototype object, so spread before comparing.
    assert.deepEqual({ ...parseCommandArgs(echo, ['--text', 'a']).values }, { text: 'a' });
    assert.throws(() => parseCommandArgs(echo, ['--loud=yes']), UsageError);
  });
});

describe('help formatting', () => {
  it('aligns the command list', () => {
    const text = formatOverviewHelp('repo', [
      echo,
      { name: 'longer-name', summary: 'Second', run: () => {} },
    ]);
    assert.match(text, /\n {2}echo {9}Echo/);
    assert.match(text, /\n {2}longer-name {2}Second/);
  });

  it('always documents --help, even for a command with no options', () => {
    const text = formatCommandHelp('repo', { name: 'bare', summary: 'Nothing', run: () => {} });
    assert.match(text, /-h, --help/);
    assert.match(text, /repo bare \[options\]/);
  });
});
