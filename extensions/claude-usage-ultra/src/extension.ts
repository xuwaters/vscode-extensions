import * as fs from 'node:fs';
import * as path from 'node:path';
import * as vscode from 'vscode';
import {
  claudeSettingsPath,
  existingStatusLine,
  manualSnippet,
  patchSettings,
  renderBridgeScript,
  unpatchSettings,
  type StatusLineSetting,
} from './bridge';
import {
  formatStatusText,
  formatTooltip,
  severityFor,
  toPlainText,
  type FormatOptions,
  type Segment,
} from './format';
import { PayloadWatcher } from './watcher';

/** Remembers what we displaced in settings.json so uninstall can restore it. */
const REPLACED_KEY = 'claudeUsageUltra.replacedStatusLine';

const DEFAULT_SEGMENTS: Segment[] = ['session', 'weekly', 'reset'];
const VALID_SEGMENTS = new Set<Segment>([
  'session',
  'weekly',
  'reset',
  'cost',
  'context',
  'model',
]);

/** Keeps the reset countdown moving between payloads. */
const TICK_MS = 30_000;

function readOptions(): FormatOptions {
  const config = vscode.workspace.getConfiguration('claudeUsageUltra');
  const raw = config.get<string[]>('show', DEFAULT_SEGMENTS);
  const segments = raw.filter((s): s is Segment => VALID_SEGMENTS.has(s as Segment));

  return {
    segments: segments.length > 0 ? segments : DEFAULT_SEGMENTS,
    staleAfterMs: config.get<number>('staleAfterMinutes', 20) * 60_000,
    warnAtPercent: config.get<number>('warnAtPercent', 80),
    errorAtPercent: config.get<number>('errorAtPercent', 95),
  };
}

export function activate(context: vscode.ExtensionContext): void {
  const stateDir = path.join(context.globalStorageUri.fsPath, 'statusline');
  const scriptPath = path.join(context.globalStorageUri.fsPath, 'claude-usage-bridge.sh');

  let item = createItem();
  let tick: ReturnType<typeof setInterval> | undefined;

  const watcher = new PayloadWatcher(stateDir, () => render());
  context.subscriptions.push({ dispose: () => watcher.dispose() });

  function createItem(): vscode.StatusBarItem {
    const config = vscode.workspace.getConfiguration('claudeUsageUltra');
    const alignment =
      config.get<string>('alignment', 'right') === 'left'
        ? vscode.StatusBarAlignment.Left
        : vscode.StatusBarAlignment.Right;
    const created = vscode.window.createStatusBarItem(
      'claudeUsageUltra.status',
      alignment,
      config.get<number>('priority', 100),
    );
    created.name = 'Claude Usage Ultra';
    created.command = 'claudeUsageUltra.showDetails';
    created.show();
    return created;
  }

  function bridgeInstalled(): boolean {
    return fs.existsSync(scriptPath);
  }

  function render(): void {
    const options = readOptions();
    const snapshot = watcher.snapshot;
    const now = Date.now();

    item.text = formatStatusText(snapshot, now, options);

    const tooltip = new vscode.MarkdownString(
      formatTooltip(snapshot, now, options, bridgeInstalled()),
    );
    tooltip.supportThemeIcons = true;
    item.tooltip = tooltip;

    switch (severityFor(snapshot, options)) {
      case 'error':
        item.backgroundColor = new vscode.ThemeColor('statusBarItem.errorBackground');
        break;
      case 'warning':
        item.backgroundColor = new vscode.ThemeColor('statusBarItem.warningBackground');
        break;
      default:
        item.backgroundColor = undefined;
    }
  }

  async function install(): Promise<void> {
    const config = vscode.workspace.getConfiguration('claudeUsageUltra');
    const refreshInterval = config.get<number>('refreshInterval', 10);
    const settingsPath = claudeSettingsPath();

    let source: string | undefined;
    try {
      source = fs.readFileSync(settingsPath, 'utf8');
    } catch {
      source = undefined;
    }

    let previous: StatusLineSetting | undefined;
    try {
      previous = existingStatusLine(source);
    } catch {
      await showManualFallback(
        `Could not parse ${settingsPath}. Add this yourself:`,
        scriptPath,
        refreshInterval,
      );
      return;
    }

    let delegate = '';
    if (previous && previous.command !== scriptPath) {
      const choice = await vscode.window.showWarningMessage(
        'Claude Code already has a status line command configured.',
        { modal: true, detail: `Current: ${previous.command}` },
        'Chain it',
        'Replace it',
      );
      if (choice === undefined) return;
      if (choice === 'Chain it') delegate = previous.command;
    } else {
      const confirmed = await vscode.window.showInformationMessage(
        'Install the Claude Code usage bridge?',
        {
          modal: true,
          detail:
            `Writes ${scriptPath}\n` +
            `Sets "statusLine" in ${settingsPath}\n\n` +
            'Claude Code will pipe its status line payload to that script, which saves it for this extension to read.',
        },
        'Install',
      );
      if (confirmed !== 'Install') return;
    }

    try {
      fs.mkdirSync(path.dirname(scriptPath), { recursive: true });
      fs.mkdirSync(stateDir, { recursive: true });
      fs.writeFileSync(scriptPath, renderBridgeScript(stateDir, delegate), { mode: 0o755 });
      fs.chmodSync(scriptPath, 0o755);

      if (source !== undefined) {
        fs.copyFileSync(settingsPath, `${settingsPath}.wx-vsce-claude-usage-ultra.bak`);
      } else {
        fs.mkdirSync(path.dirname(settingsPath), { recursive: true });
      }

      const patched = patchSettings(source, scriptPath, refreshInterval);
      fs.writeFileSync(settingsPath, patched.json, 'utf8');
      await context.globalState.update(REPLACED_KEY, delegate ? patched.replaced : undefined);
    } catch (error) {
      vscode.window.showErrorMessage(`Claude Usage Ultra: install failed — ${describe(error)}`);
      return;
    }

    watcher.refresh(true);
    render();
    vscode.window.showInformationMessage(
      'Claude usage bridge installed. Start or continue a Claude Code session in a terminal to populate the status bar.',
    );
  }

  async function uninstall(): Promise<void> {
    const settingsPath = claudeSettingsPath();
    const confirmed = await vscode.window.showWarningMessage(
      'Remove the Claude Code usage bridge?',
      { modal: true, detail: `Deletes ${scriptPath} and clears "statusLine" in ${settingsPath}.` },
      'Remove',
    );
    if (confirmed !== 'Remove') return;

    try {
      const restore = context.globalState.get<StatusLineSetting>(REPLACED_KEY);
      const source = fs.readFileSync(settingsPath, 'utf8');
      const result = unpatchSettings(source, scriptPath, restore);
      if (result.changed) fs.writeFileSync(settingsPath, result.json, 'utf8');
      await context.globalState.update(REPLACED_KEY, undefined);
    } catch (error) {
      vscode.window.showWarningMessage(
        `Claude Usage Ultra: could not update settings.json — ${describe(error)}`,
      );
    }

    try {
      fs.rmSync(scriptPath, { force: true });
      fs.rmSync(path.join(stateDir, 'current.json'), { force: true });
    } catch {
      // Nothing actionable; the settings change is what matters.
    }

    watcher.refresh(true);
    render();
    vscode.window.showInformationMessage('Claude usage bridge removed.');
  }

  async function showManualFallback(
    message: string,
    script: string,
    refreshInterval: number,
  ): Promise<void> {
    const snippet = manualSnippet(script, refreshInterval);
    const choice = await vscode.window.showErrorMessage(
      message,
      { modal: true, detail: snippet },
      'Copy snippet',
    );
    if (choice === 'Copy snippet') await vscode.env.clipboard.writeText(snippet);
  }

  async function showDetails(): Promise<void> {
    const installed = bridgeInstalled();
    const snapshot = watcher.snapshot;
    const options = readOptions();

    const detail = toPlainText(formatTooltip(snapshot, Date.now(), options, installed));

    const actions = installed
      ? ['Refresh', 'Reinstall bridge', 'Remove bridge']
      : ['Install bridge'];
    const choice = await vscode.window.showInformationMessage(detail, ...actions);

    if (choice === 'Refresh') {
      watcher.refresh(true);
      render();
    } else if (choice === 'Install bridge' || choice === 'Reinstall bridge') {
      await install();
    } else if (choice === 'Remove bridge') {
      await uninstall();
    }
  }

  context.subscriptions.push(
    // Disposes whichever item is current — alignment changes replace it.
    { dispose: () => item.dispose() },
    vscode.commands.registerCommand('claudeUsageUltra.install', install),
    vscode.commands.registerCommand('claudeUsageUltra.uninstall', uninstall),
    vscode.commands.registerCommand('claudeUsageUltra.showDetails', showDetails),
    vscode.commands.registerCommand('claudeUsageUltra.refresh', () => {
      watcher.refresh(true);
      render();
    }),
    vscode.workspace.onDidChangeConfiguration((event) => {
      if (!event.affectsConfiguration('claudeUsageUltra')) return;
      if (
        event.affectsConfiguration('claudeUsageUltra.alignment') ||
        event.affectsConfiguration('claudeUsageUltra.priority')
      ) {
        item.dispose();
        item = createItem();
      }
      if (event.affectsConfiguration('claudeUsageUltra.pollIntervalSeconds')) {
        watcher.setPollInterval(pollIntervalMs());
      }
      render();
    }),
  );

  function pollIntervalMs(): number {
    return vscode.workspace.getConfiguration('claudeUsageUltra').get<number>('pollIntervalSeconds', 15) * 1000;
  }

  watcher.start(pollIntervalMs());
  render();

  tick = setInterval(render, TICK_MS);
  context.subscriptions.push({
    dispose: () => {
      if (tick) clearInterval(tick);
      tick = undefined;
    },
  });
}

function describe(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function deactivate(): void {}
