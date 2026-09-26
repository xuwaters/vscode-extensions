import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { describe, expect, it } from 'vitest';
import { bundledCliPath, CLI_ARGS, fetchUsage, UsageCliError } from './cli';

// Vitest runs with the package as its working directory.
const STUB = path.resolve('src/testdata/stub-cli.mjs');

/** Run the stub CLI in the given mode, through the real spawn path. */
function runStub(mode: string, overrides: Parameters<typeof fetchUsage>[0] | object = {}) {
  return fetchUsage({
    command: process.execPath,
    argsPrefix: [STUB],
    env: { ...process.env, STUB_MODE: mode },
    timeoutMs: 10_000,
    ...overrides,
  });
}

describe('fetchUsage', () => {
  it('has a stub to run', () => {
    expect(fs.existsSync(STUB), `stub CLI missing at ${STUB}`).toBe(true);
  });

  it('initializes, asks for usage, and returns the raw response', async () => {
    const response = (await runStub('ok')) as { subscription_type: string };
    expect(response.subscription_type).toBe('max');
  });

  it('passes the flags that keep CLI startup cheap', async () => {
    const response = (await runStub('ok')) as { argv: string[] };
    expect(response.argv).toEqual([...CLI_ARGS]);
    expect(response.argv).toContain('--strict-mcp-config');
    expect(response.argv).toContain('--no-session-persistence');
  });

  it('runs in the requested directory', async () => {
    const cwd = os.tmpdir();
    const response = (await runStub('ok', { cwd })) as { cwd: string };
    // macOS reports /var as /private/var, so compare what the OS resolved.
    expect(path.basename(response.cwd)).toBe(path.basename(cwd));
  });

  it('ignores the debug and progress lines the real CLI interleaves', async () => {
    const response = (await runStub('noise')) as { subscription_type: string };
    expect(response.subscription_type).toBe('max');
  });

  it('surfaces an error response from the CLI', async () => {
    await expect(runStub('error')).rejects.toThrow('usage unavailable');
  });

  it('gives up when the CLI never answers', async () => {
    const failure = runStub('silent', { timeoutMs: 300 });
    await expect(failure).rejects.toThrow(/did not answer within 300ms/);
  });

  it('reports a CLI that quits early', async () => {
    await expect(runStub('exit')).rejects.toThrow(/exited early/);
  });

  it('flags a missing executable so the caller can explain the fix', async () => {
    const failure = fetchUsage({
      command: path.join(os.tmpdir(), 'claude-usage-ultra-does-not-exist'),
      timeoutMs: 5_000,
    });
    await expect(failure).rejects.toMatchObject({ missingCli: true });
    await expect(failure).rejects.toBeInstanceOf(UsageCliError);
  });
});

describe('cli path helpers', () => {
  it('points at the binary the Claude Code extension bundles', () => {
    expect(bundledCliPath('/ext/anthropic.claude-code', 'darwin')).toBe(
      '/ext/anthropic.claude-code/resources/native-binary/claude',
    );
  });

  it('adds the extension on Windows', () => {
    expect(bundledCliPath('C:\\ext', 'win32')).toContain('claude.exe');
  });
});
