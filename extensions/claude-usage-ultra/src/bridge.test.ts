import { execFileSync } from 'node:child_process';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import {
  claudeConfigDir,
  claudeSettingsPath,
  existingStatusLine,
  manualSnippet,
  patchSettings,
  renderBridgeScript,
  shQuote,
  unpatchSettings,
} from './bridge';
import { parseSnapshot } from './payload';

const PAYLOAD = {
  session_id: 'abc',
  model: { id: 'claude-opus-5', display_name: 'Opus 5' },
  cost: { total_cost_usd: 1.25 },
  context_window: { context_window_size: 200_000, used_percentage: 26.5 },
  rate_limits: {
    five_hour: { used_percentage: 1, resets_at: '2026-08-05T13:07:00.000Z' },
    seven_day: { used_percentage: 68, resets_at: '2026-08-09T00:00:00.000Z' },
  },
};

const tempDirs: string[] = [];

function tempDir(): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'claude-usage-'));
  tempDirs.push(dir);
  return dir;
}

afterEach(() => {
  while (tempDirs.length > 0) {
    fs.rmSync(tempDirs.pop()!, { recursive: true, force: true });
  }
});

/** Write the script and feed it a payload exactly as Claude Code would. */
function runBridge(stateDir: string, delegate = ''): string {
  const scriptPath = path.join(tempDir(), 'bridge.sh');
  fs.writeFileSync(scriptPath, renderBridgeScript(stateDir, delegate), { mode: 0o755 });
  return execFileSync('sh', [scriptPath], {
    input: JSON.stringify(PAYLOAD),
    encoding: 'utf8',
  });
}

describe('shQuote', () => {
  it('survives spaces and embedded quotes', () => {
    expect(shQuote('/tmp/plain')).toBe(`'/tmp/plain'`);
    expect(execFileSync('sh', ['-c', `printf '%s' ${shQuote("a b'c$d")}`], { encoding: 'utf8' })).toBe(
      "a b'c$d",
    );
  });
});

describe('renderBridgeScript', () => {
  it('persists a payload the extension can parse', () => {
    const stateDir = path.join(tempDir(), 'statusline');
    runBridge(stateDir);

    const saved = fs.readFileSync(path.join(stateDir, 'current.json'), 'utf8');
    const snapshot = parseSnapshot(saved, 0);
    expect(snapshot!.fiveHour?.usedPercent).toBe(1);
    expect(snapshot!.sevenDay?.usedPercent).toBe(68);
    expect(snapshot!.modelName).toBe('Opus 5');
  });

  it('creates the state directory on first run', () => {
    const stateDir = path.join(tempDir(), 'nested', 'statusline');
    runBridge(stateDir);
    expect(fs.existsSync(path.join(stateDir, 'current.json'))).toBe(true);
  });

  it('leaves no temp files behind', () => {
    const stateDir = path.join(tempDir(), 'statusline');
    runBridge(stateDir);
    expect(fs.readdirSync(stateDir)).toEqual(['current.json']);
  });

  it('renders a usable terminal line', () => {
    const stdout = runBridge(path.join(tempDir(), 'statusline'));
    expect(stdout).toBe('Opus 5 | 5h 1% | 7d 68%');
  });

  it('delegates rendering, replaying the payload on stdin', () => {
    const stateDir = path.join(tempDir(), 'statusline');
    const stdout = runBridge(stateDir, `sed -n 's/.*"session_id":"\\([^"]*\\)".*/session=\\1/p'`);
    expect(stdout.trim()).toBe('session=abc');
    // Delegating must not skip persisting the payload.
    expect(fs.existsSync(path.join(stateDir, 'current.json'))).toBe(true);
  });

  it('handles a state directory path containing spaces and quotes', () => {
    const stateDir = path.join(tempDir(), "odd dir's name");
    runBridge(stateDir);
    expect(fs.existsSync(path.join(stateDir, 'current.json'))).toBe(true);
  });
});

describe('patchSettings', () => {
  it('adds statusLine while preserving unrelated settings', () => {
    const source = JSON.stringify({ model: 'opus', permissions: { allow: ['Bash(ls:*)'] } });
    const result = patchSettings(source, '/scripts/bridge.sh', 10);
    const parsed = JSON.parse(result.json);

    expect(parsed.model).toBe('opus');
    expect(parsed.permissions.allow).toEqual(['Bash(ls:*)']);
    expect(parsed.statusLine).toEqual({
      type: 'command',
      command: '/scripts/bridge.sh',
      refreshInterval: 10,
    });
    expect(result.replaced).toBeUndefined();
  });

  it('handles a missing or empty settings file', () => {
    expect(JSON.parse(patchSettings(undefined, '/s.sh', 5).json).statusLine.command).toBe('/s.sh');
    expect(JSON.parse(patchSettings('   ', '/s.sh', 5).json).statusLine.command).toBe('/s.sh');
  });

  it('reports the status line it displaced', () => {
    const source = JSON.stringify({ statusLine: { type: 'command', command: 'my-line.sh' } });
    const result = patchSettings(source, '/scripts/bridge.sh', 10);
    expect(result.replaced).toEqual({ type: 'command', command: 'my-line.sh' });
  });

  it('does not report itself as displaced when reinstalling', () => {
    const source = JSON.stringify({ statusLine: { type: 'command', command: '/scripts/bridge.sh' } });
    expect(patchSettings(source, '/scripts/bridge.sh', 10).replaced).toBeUndefined();
  });

  it('throws on unparseable settings rather than clobbering them', () => {
    expect(() => patchSettings('{ not json', '/s.sh', 5)).toThrow();
    expect(() => patchSettings('[]', '/s.sh', 5)).toThrow();
  });
});

describe('unpatchSettings', () => {
  it('removes only our own status line', () => {
    const source = patchSettings(JSON.stringify({ model: 'opus' }), '/s.sh', 10).json;
    const result = unpatchSettings(source, '/s.sh');
    expect(result.changed).toBe(true);
    expect(JSON.parse(result.json).statusLine).toBeUndefined();
    expect(JSON.parse(result.json).model).toBe('opus');
  });

  it('restores a chained status line', () => {
    const source = patchSettings(JSON.stringify({}), '/s.sh', 10).json;
    const restored = unpatchSettings(source, '/s.sh', { type: 'command', command: 'mine.sh' });
    expect(JSON.parse(restored.json).statusLine).toEqual({ type: 'command', command: 'mine.sh' });
  });

  it('leaves someone else’s status line alone', () => {
    const source = JSON.stringify({ statusLine: { type: 'command', command: 'other.sh' } });
    const result = unpatchSettings(source, '/s.sh');
    expect(result.changed).toBe(false);
    expect(result.json).toBe(source);
  });
});

describe('existingStatusLine', () => {
  it('reads a configured command, ignoring malformed entries', () => {
    expect(existingStatusLine(JSON.stringify({ statusLine: { type: 'command', command: 'x' } }))).toEqual(
      { type: 'command', command: 'x' },
    );
    expect(existingStatusLine(JSON.stringify({ statusLine: { type: 'command' } }))).toBeUndefined();
    expect(existingStatusLine(undefined)).toBeUndefined();
  });
});

describe('claudeConfigDir', () => {
  it('defaults to ~/.claude and honours CLAUDE_CONFIG_DIR', () => {
    expect(claudeConfigDir({} as NodeJS.ProcessEnv)).toBe(path.join(os.homedir(), '.claude'));
    expect(claudeConfigDir({ CLAUDE_CONFIG_DIR: '/custom/claude' } as NodeJS.ProcessEnv)).toBe(
      '/custom/claude',
    );
    expect(claudeSettingsPath({ CLAUDE_CONFIG_DIR: '/custom/claude' } as NodeJS.ProcessEnv)).toBe(
      '/custom/claude/settings.json',
    );
  });
});

describe('manualSnippet', () => {
  it('is valid JSON the user can paste', () => {
    expect(JSON.parse(manualSnippet('/s.sh', 10)).statusLine.command).toBe('/s.sh');
  });
});
