import * as fs from 'node:fs';
import * as vscode from 'vscode';
import { bundledCliPath, fetchUsage, UsageCliError } from './cli';
import {
  LOADING_TEXT,
  formatStatusText,
  formatTooltip,
  toPlainText,
  type FormatOptions,
  type Segment,
} from './format';
import { toSnapshot, type UsageSnapshot } from './usage';

/** Last good reading, so a reload does not start with a blank bar. */
const SNAPSHOT_KEY = 'claudeUsageUltra.lastSnapshot';

/** The Claude Code extension, whose bundled CLI we borrow. */
const CLAUDE_CODE_EXTENSION_ID = 'Anthropic.claude-code';

/**
 * Anthropic's own usage page — the authority these numbers are read against
 * when a reading looks wrong, or has gone stale and the bar cannot be trusted.
 */
const USAGE_SETTINGS_URL = 'https://claude.ai/settings/usage';

const DEFAULT_SEGMENTS: Segment[] = ['plan', 'session', 'weekly', 'scoped', 'spend', 'reset'];
const VALID_SEGMENTS = new Set<Segment>([
  'session',
  'weekly',
  'scoped',
  'reset',
  'spend',
  'plan',
]);

/** Keeps the reset countdown moving between refreshes. */
const TICK_MS = 30_000;

/** Refresh on focus only if the reading is older than this. */
const FOCUS_REFRESH_AFTER_MS = 60_000;

/** Failed refreshes back off from the configured interval up to this ceiling. */
const MAX_BACKOFF_MS = 30 * 60_000;

function config(): vscode.WorkspaceConfiguration {
  return vscode.workspace.getConfiguration('claudeUsageUltra');
}

function readOptions(): FormatOptions {
  const settings = config();
  const raw = settings.get<string[]>('show', DEFAULT_SEGMENTS);
  const segments = raw.filter((s): s is Segment => VALID_SEGMENTS.has(s as Segment));

  return {
    segments: segments.length > 0 ? segments : DEFAULT_SEGMENTS,
    label: settings.get<string>('label', 'Claude'),
    staleAfterMs: settings.get<number>('staleAfterMinutes', 30) * 60_000,
    noticeAtPercent: settings.get<number>('noticeAtPercent', 50),
    warnAtPercent: settings.get<number>('warnAtPercent', 80),
    criticalAtPercent: criticalAtPercent(settings),
  };
}

/** The value a user actually wrote down, at whichever scope they wrote it. */
function explicit(settings: vscode.WorkspaceConfiguration, key: string): number | undefined {
  const set = settings.inspect<number>(key);
  return set?.workspaceFolderValue ?? set?.workspaceValue ?? set?.globalValue;
}

/**
 * The red dot's threshold. It was once `errorAtPercent` — a full window is not
 * an error — so a threshold moved under the old name still counts until it is
 * moved under the new one.
 */
function criticalAtPercent(settings: vscode.WorkspaceConfiguration): number {
  return (
    explicit(settings, 'criticalAtPercent') ??
    explicit(settings, 'errorAtPercent') ??
    settings.get<number>('criticalAtPercent', 95)
  );
}

/**
 * The Claude Code CLI to query: only ever the binary shipped inside the
 * installed Claude Code extension.
 *
 * Nothing a workspace controls can choose the executable — no setting, no
 * PATH lookup — so opening a repository can never make this extension run a
 * program that repository supplied.
 */
function resolveCli(log: vscode.LogOutputChannel): string | undefined {
  const claudeCode = vscode.extensions.getExtension(CLAUDE_CODE_EXTENSION_ID);
  if (!claudeCode) return undefined;
  const bundled = bundledCliPath(claudeCode.extensionUri.fsPath);
  if (fs.existsSync(bundled)) return bundled;
  log.debug(`Claude Code extension found but no bundled CLI at ${bundled}`);
  return undefined;
}

const MISSING_CLI = 'Could not find the Claude Code CLI. Install the Claude Code extension.';

export function activate(context: vscode.ExtensionContext): void {
  const log = vscode.window.createOutputChannel('Claude Usage Ultra', { log: true });
  context.subscriptions.push(log);

  let item = createItem();
  let snapshot = context.globalState.get<UsageSnapshot>(SNAPSHOT_KEY);
  let problem: string | undefined;
  let inFlight = false;
  let everRefreshed = false;
  let failures = 0;
  let pollTimer: ReturnType<typeof setTimeout> | undefined;
  let tickTimer: ReturnType<typeof setInterval> | undefined;

  function createItem(): vscode.StatusBarItem {
    const settings = config();
    const alignment =
      settings.get<string>('alignment', 'right') === 'left'
        ? vscode.StatusBarAlignment.Left
        : vscode.StatusBarAlignment.Right;
    const created = vscode.window.createStatusBarItem(
      'claudeUsageUltra.status',
      alignment,
      settings.get<number>('priority', 100),
    );
    created.name = 'Claude Usage Ultra';
    created.command = 'claudeUsageUltra.showDetails';
    created.show();
    return created;
  }

  function render(): void {
    const options = readOptions();
    const now = Date.now();

    item.text =
      snapshot === undefined && inFlight ? LOADING_TEXT : formatStatusText(snapshot, now, options);

    const tooltip = new vscode.MarkdownString(formatTooltip(snapshot, now, options, problem));
    tooltip.supportThemeIcons = true;
    item.tooltip = tooltip;

    // No background colour: it would paint every window with the severity of
    // the worst one. The dots inside the text carry severity per window, which
    // is the whole reason they are emoji rather than codicons.
  }

  function pollIntervalMs(): number {
    return Math.max(30, config().get<number>('pollIntervalSeconds', 300)) * 1000;
  }

  /** Schedule the next refresh, backing off while the CLI keeps failing. */
  function schedule(): void {
    if (pollTimer) clearTimeout(pollTimer);
    const base = pollIntervalMs();
    const delay = failures === 0 ? base : Math.min(base * 2 ** failures, MAX_BACKOFF_MS);
    pollTimer = setTimeout(() => void refresh(), delay);
  }

  async function refresh(): Promise<void> {
    if (inFlight) return;
    inFlight = true;
    everRefreshed = true;
    render();

    const command = resolveCli(log);
    const started = Date.now();

    try {
      if (!command) throw new UsageCliError(MISSING_CLI, true);
      // Run outside any workspace: a repository's own `.claude/settings.json`
      // (hooks, env, apiKeyHelper) must not load into a background query.
      // Global storage is private to this extension and holds no project.
      const cwd = context.globalStorageUri.fsPath;
      await fs.promises.mkdir(cwd, { recursive: true });
      const response = await fetchUsage({
        command,
        cwd,
        timeoutMs: config().get<number>('timeoutSeconds', 45) * 1000,
        log: (message) => log.debug(message),
      });

      const next = toSnapshot(response, Date.now());
      if (!next) throw new UsageCliError('Claude Code returned an unrecognised usage payload');

      snapshot = next;
      problem = undefined;
      failures = 0;
      await context.globalState.update(SNAPSHOT_KEY, next);
      log.info(
        `Refreshed in ${Date.now() - started}ms — ${next.limits
          .map((l) => `${l.kind} ${Math.round(l.percent)}%`)
          .join(', ')}`,
      );
    } catch (error) {
      failures += 1;
      problem =
        error instanceof UsageCliError && error.missingCli
          ? MISSING_CLI
          : `Last refresh failed: ${describe(error)}`;
      log.warn(`${problem} (attempt ${failures}, ${command ?? 'no CLI'})`);
    } finally {
      inFlight = false;
      render();
      schedule();
    }
  }

  function openUsagePage(): Thenable<boolean> {
    return vscode.env.openExternal(vscode.Uri.parse(USAGE_SETTINGS_URL));
  }

  async function showDetails(): Promise<void> {
    const detail = toPlainText(formatTooltip(snapshot, Date.now(), readOptions(), problem));
    const choice = await vscode.window.showInformationMessage(
      detail,
      'Refresh',
      'Check on claude.ai',
      'Show log',
    );
    if (choice === 'Refresh') await refresh();
    else if (choice === 'Check on claude.ai') await openUsagePage();
    else if (choice === 'Show log') log.show();
  }

  context.subscriptions.push(
    // Disposes whichever item is current — alignment changes replace it.
    { dispose: () => item.dispose() },
    vscode.commands.registerCommand('claudeUsageUltra.refresh', () => refresh()),
    vscode.commands.registerCommand('claudeUsageUltra.showDetails', showDetails),
    vscode.commands.registerCommand('claudeUsageUltra.openUsagePage', () => openUsagePage()),
    vscode.commands.registerCommand('claudeUsageUltra.showLog', () => log.show()),
    vscode.workspace.onDidChangeConfiguration((event) => {
      if (!event.affectsConfiguration('claudeUsageUltra')) return;
      if (
        event.affectsConfiguration('claudeUsageUltra.alignment') ||
        event.affectsConfiguration('claudeUsageUltra.priority')
      ) {
        item.dispose();
        item = createItem();
      }
      if (event.affectsConfiguration('claudeUsageUltra.pollIntervalSeconds')) schedule();
      render();
    }),
    vscode.window.onDidChangeWindowState((state) => {
      if (!state.focused) return;
      if (!config().get<boolean>('refreshOnFocus', true)) return;
      const age = snapshot ? Date.now() - snapshot.fetchedAtMs : Number.POSITIVE_INFINITY;
      if (age > FOCUS_REFRESH_AFTER_MS) void refresh();
    }),
    {
      dispose: () => {
        if (pollTimer) clearTimeout(pollTimer);
        if (tickTimer) clearInterval(tickTimer);
      },
    },
  );

  render();
  void refresh();

  tickTimer = setInterval(() => {
    // Only the countdowns move between refreshes; skip the work if nothing is
    // on screen yet.
    if (snapshot !== undefined || everRefreshed) render();
  }, TICK_MS);
}

function describe(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function deactivate(): void {}
