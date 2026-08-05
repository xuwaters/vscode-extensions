import { spawn, type ChildProcessByStdio } from 'node:child_process';
import * as os from 'node:os';
import * as path from 'node:path';
import type { Readable, Writable } from 'node:stream';

/**
 * Asks the Claude Code CLI for plan usage over its stream-JSON control
 * protocol — the same request the Claude Code extension makes on behalf of its
 * Usage panel.
 *
 * The CLI owns the OAuth credentials and the call to `/api/oauth/usage`, so
 * this extension never reads a token or talks to the API itself. In exchange we
 * pay for a short-lived process, which the flags below keep to about a second:
 * an empty MCP config skips connecting the user's MCP servers (by far the
 * slowest part of startup) and session persistence is pointless for a process
 * that never runs a turn.
 */
export const CLI_ARGS: readonly string[] = [
  '--output-format',
  'stream-json',
  '--verbose',
  '--input-format',
  'stream-json',
  '--strict-mcp-config',
  '--mcp-config',
  '{"mcpServers":{}}',
  '--no-session-persistence',
];

/**
 * The CLI answers `initialize` with every slash command, agent and skill it
 * knows about, which is already tens of kilobytes. Cap the buffer so a
 * pathological response cannot grow without bound.
 */
const MAX_STDOUT_BYTES = 8 * 1024 * 1024;

/** How long to let the process shut down politely before killing it. */
const KILL_GRACE_MS = 2_000;

export interface FetchUsageOptions {
  /** Executable to run — a bundled binary path, or `claude` from PATH. */
  command: string;
  /** Prepended to {@link CLI_ARGS}; tests use it to point node at a stub. */
  argsPrefix?: readonly string[];
  cwd?: string;
  timeoutMs?: number;
  env?: NodeJS.ProcessEnv;
  log?: (message: string) => void;
}

export class UsageCliError extends Error {
  constructor(
    message: string,
    /** True when the executable itself could not be run. */
    readonly missingCli = false,
  ) {
    super(message);
    this.name = 'UsageCliError';
  }
}

interface ControlResponse {
  type?: string;
  response?: {
    subtype?: string;
    request_id?: string;
    response?: unknown;
    error?: string;
  };
}

/** Where the Claude Code extension keeps the CLI it ships with. */
export function bundledCliPath(extensionPath: string, platform: string = process.platform): string {
  const binary = platform === 'win32' ? 'claude.exe' : 'claude';
  return path.join(extensionPath, 'resources', 'native-binary', binary);
}

/** The CLI a `claude` install puts in the user's home directory. */
export function localCliPath(
  platform: string = process.platform,
  homedir: string = os.homedir(),
): string {
  const binary = platform === 'win32' ? 'claude.exe' : 'claude';
  return path.join(homedir, '.claude', 'local', binary);
}

/**
 * Run one usage query.
 *
 * Resolves with the raw `get_usage` response so parsing stays in `usage.ts`
 * and this module has no opinion about the payload's shape.
 */
export function fetchUsage(options: FetchUsageOptions): Promise<unknown> {
  const { command, argsPrefix = [], cwd, timeoutMs = 30_000, env, log } = options;

  return new Promise<unknown>((resolve, reject) => {
    // stderr is dropped: the CLI writes progress chatter there, never the answer.
    let child: ChildProcessByStdio<Writable, Readable, null>;
    try {
      child = spawn(command, [...argsPrefix, ...CLI_ARGS], {
        cwd,
        env,
        stdio: ['pipe', 'pipe', 'ignore'],
        windowsHide: true,
      });
    } catch (error) {
      reject(new UsageCliError(`Could not start ${command}: ${describe(error)}`, true));
      return;
    }

    let settled = false;
    let stdout = '';
    let killTimer: ReturnType<typeof setTimeout> | undefined;

    const timeout = setTimeout(() => {
      finish(new UsageCliError(`${command} did not answer within ${timeoutMs}ms`));
    }, timeoutMs);

    function finish(error: UsageCliError | undefined, value?: unknown): void {
      if (settled) return;
      settled = true;
      clearTimeout(timeout);

      // SIGTERM lets the CLI flush; SIGKILL is the backstop if it does not.
      child.kill('SIGTERM');
      killTimer = setTimeout(() => child.kill('SIGKILL'), KILL_GRACE_MS);
      killTimer.unref?.();

      if (error) reject(error);
      else resolve(value);
    }

    function send(request: Record<string, unknown>, requestId: string): void {
      const message = JSON.stringify({ type: 'control_request', request_id: requestId, request });
      // The process can die between our check and the write; a failed write is
      // reported by the 'error'/'exit' handlers, so swallow it here.
      child.stdin.write(`${message}\n`, () => {});
    }

    child.on('error', (error) => {
      const code = (error as NodeJS.ErrnoException).code;
      finish(
        new UsageCliError(`Could not run ${command}: ${error.message}`, code === 'ENOENT'),
      );
    });

    child.stdin.on('error', () => {
      // The exit handler reports why.
    });

    child.on('exit', (code, signal) => {
      if (killTimer) clearTimeout(killTimer);
      finish(new UsageCliError(`${command} exited early (code ${code ?? signal})`));
    });

    child.stdout.setEncoding('utf8');
    child.stdout.on('data', (chunk: string) => {
      if (stdout.length > MAX_STDOUT_BYTES) return;
      stdout += chunk;

      let newline: number;
      while ((newline = stdout.indexOf('\n')) >= 0) {
        const line = stdout.slice(0, newline);
        stdout = stdout.slice(newline + 1);
        if (line.trim()) handleLine(line);
        if (settled) return;
      }
    });

    function handleLine(line: string): void {
      let message: ControlResponse;
      try {
        message = JSON.parse(line) as ControlResponse;
      } catch {
        return; // Progress and debug lines are not our business.
      }
      if (message.type !== 'control_response') return;

      const response = message.response;
      if (response?.subtype === 'error') {
        finish(new UsageCliError(response.error ?? 'Claude Code refused the request'));
        return;
      }
      if (response?.subtype !== 'success') return;

      if (response.request_id === INIT_ID) {
        log?.('initialized, requesting usage');
        send({ subtype: 'get_usage' }, USAGE_ID);
        return;
      }
      if (response.request_id === USAGE_ID) {
        finish(undefined, response.response);
      }
    }

    // The CLI ignores every other control request until it has initialized.
    send({ subtype: 'initialize' }, INIT_ID);
  });
}

const INIT_ID = 'usage-ultra-init';
const USAGE_ID = 'usage-ultra-get';

function describe(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
