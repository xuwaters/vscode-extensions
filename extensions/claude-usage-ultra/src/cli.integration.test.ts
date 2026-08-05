import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { describe, expect, it } from 'vitest';
import { bundledCliPath, fetchUsage, localCliPath } from './cli';
import { formatStatusText, formatTooltip, type FormatOptions } from './format';
import { toSnapshot } from './usage';

/**
 * Exercises the whole pipeline against the real Claude Code CLI: spawn, control
 * protocol, parse, render. It needs a signed-in CLI and network access, so it
 * is opt-in:
 *
 *   CLAUDE_USAGE_ULTRA_E2E=1 pnpm test
 *
 * Run it after touching `CLI_ARGS` or the control-protocol handshake — those
 * are the parts a stub cannot keep honest.
 */
const enabled = process.env.CLAUDE_USAGE_ULTRA_E2E === '1';

/** The same search order `resolveCli` uses, minus the VS Code lookup. */
function findCli(): string | undefined {
  const configured = process.env.CLAUDE_USAGE_ULTRA_CLI;
  if (configured) return configured;

  const extensionsDir = path.join(os.homedir(), '.vscode', 'extensions');
  const claudeCode = fs.existsSync(extensionsDir)
    ? fs
        .readdirSync(extensionsDir)
        .filter((name) => name.toLowerCase().startsWith('anthropic.claude-code-'))
        .sort()
        .reverse()
        .map((name) => bundledCliPath(path.join(extensionsDir, name)))
        .find((candidate) => fs.existsSync(candidate))
    : undefined;
  if (claudeCode) return claudeCode;

  const local = localCliPath();
  return fs.existsSync(local) ? local : undefined;
}

const OPTIONS: FormatOptions = {
  segments: ['plan', 'session', 'weekly', 'scoped', 'spend', 'reset'],
  label: 'Claude',
  staleAfterMs: 30 * 60_000,
  noticeAtPercent: 50,
  warnAtPercent: 80,
  errorAtPercent: 95,
};

describe.runIf(enabled)('fetchUsage against the real CLI', () => {
  it('returns usage this extension can render', { timeout: 90_000 }, async () => {
    const cli = findCli();
    expect(cli, 'no Claude Code CLI found; set CLAUDE_USAGE_ULTRA_CLI').toBeDefined();

    const response = await fetchUsage({ command: cli!, timeoutMs: 60_000 });
    const now = Date.now();
    const snapshot = toSnapshot(response, now);

    expect(snapshot).toBeDefined();
    expect(snapshot!.available).toBe(true);
    expect(snapshot!.limits.length).toBeGreaterThan(0);

    for (const limit of snapshot!.limits) {
      expect(limit.percent).toBeGreaterThanOrEqual(0);
      expect(limit.label).not.toBe('');
    }

    // A session window should always be present and should reset within 5 hours.
    const session = snapshot!.limits.find((limit) => limit.group === 'session');
    expect(session).toBeDefined();
    expect(session!.resetsAtMs! - now).toBeLessThanOrEqual(5 * 3_600_000 + 60_000);

    const text = formatStatusText(snapshot, now, OPTIONS);
    expect(text).toMatch(/^\$\(pulse\) Claude · \w+ · 5h \d+%/);
    expect(text).not.toContain('(stale)');
    expect(text).not.toContain('undefined');

    const tooltip = formatTooltip(snapshot, now, OPTIONS);
    expect(tooltip).toContain('Session (5h)');
    expect(tooltip).not.toContain('undefined');

    // Surfaced so a failing run shows what the CLI actually said.
    console.log(text);
    console.log(tooltip);
  });
});
