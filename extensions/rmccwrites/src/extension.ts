import * as vscode from 'vscode';
import { Cleaner, directories, type Config, type Reporter } from './cleaner.js';
import { resolveConfig, type RawSettings } from './settings.js';
import { VscodeFs } from './vscodeFs.js';

const LOG_NAME = 'Remove .cc-writes';
/** Paths spelled out in the confirmation dialog before it switches to a count. */
const MAX_LISTED = 20;

let log: vscode.LogOutputChannel;

export function activate(context: vscode.ExtensionContext): void {
  log = vscode.window.createOutputChannel(LOG_NAME, { log: true });

  context.subscriptions.push(
    log,
    vscode.commands.registerCommand('rmccwrites.clean', () => clean(workspaceRoots())),
    vscode.commands.registerCommand('rmccwrites.preview', () => preview(workspaceRoots())),
    vscode.commands.registerCommand('rmccwrites.cleanFolder', (uri?: vscode.Uri, uris?: vscode.Uri[]) =>
      clean(selectedRoots(uri, uris)),
    ),
    vscode.commands.registerCommand('rmccwrites.showLog', () => log.show(true)),
  );
}

export function deactivate(): void {}

/** Scan, confirm, then remove. */
async function clean(roots: vscode.Uri[]): Promise<void> {
  if (roots.length === 0) return;

  const scan = await walk(roots, true, 'Scanning for empty directories…');
  if (scan.cancelled) return void vscode.window.showInformationMessage('Scan cancelled.');
  if (scan.uris.length === 0) return void report(scan, true);

  if (confirmBeforeRemoving() && !(await confirm(scan.uris))) return;

  report(await walk(roots, false, 'Removing empty directories…'), false);
}

/** Scan only, then offer to remove what was found. */
async function preview(roots: vscode.Uri[]): Promise<void> {
  if (roots.length === 0) return;

  const scan = await walk(roots, true, 'Scanning for empty directories…');
  if (scan.cancelled) return void vscode.window.showInformationMessage('Scan cancelled.');
  if (scan.uris.length === 0) return void report(scan, true);

  const choice = await vscode.window.showInformationMessage(
    `${directories(scan.uris.length)} can be removed.`,
    'Remove Them',
    'Show Log',
  );
  if (choice === 'Show Log') return void log.show(true);
  if (choice !== 'Remove Them') return;

  report(await walk(roots, false, 'Removing empty directories…'), false);
}

interface RunResult {
  removed: number;
  errors: number;
  cancelled: boolean;
  /** Directories removed, or found removable during a dry run. */
  uris: vscode.Uri[];
  /** No root had any target name configured, so nothing could match. */
  noTargets: boolean;
}

/**
 * Run one pass over `roots`, each with its own folder-scoped settings, behind a
 * cancellable progress notification.
 */
function walk(roots: vscode.Uri[], dryRun: boolean, title: string): Thenable<RunResult> {
  return vscode.window.withProgress(
    { location: vscode.ProgressLocation.Notification, title, cancellable: true },
    async (progress, token) => {
      const result: RunResult = { removed: 0, errors: 0, cancelled: false, uris: [], noTargets: true };
      let found = 0;
      const reporter: Reporter = {
        say: text => {
          log.info(text);
          // Paths are long and the walk is quiet until it finds something, so
          // the notification carries the tally rather than the path.
          progress.report({ message: `${directories(++found)} so far…` });
        },
        error: text => log.error(text),
      };

      for (const root of roots) {
        if (token.isCancellationRequested) break;

        const cfg = resolveConfig(settingsFor(root), dryRun);
        if (cfg.names.length > 0) result.noTargets = false;
        log.info(describe(root, cfg));

        const fs = new VscodeFs(root);
        const summary = await new Cleaner(cfg, fs, reporter, token).run([root.path]);
        result.removed += summary.removed;
        result.errors += summary.errors;
        result.uris.push(...summary.paths.map(path => fs.uri(path)));
      }

      result.cancelled = token.isCancellationRequested;
      log.info(`${directories(result.removed)} ${dryRun ? 'removable' : 'removed'}`);
      return result;
    },
  );
}

function describe(root: vscode.Uri, cfg: Config): string {
  const list = (names: string[]) => (names.length > 0 ? names.join(', ') : '(none)');
  return [
    `${cfg.dryRun ? 'dry run' : 'cleaning'} ${root.fsPath}`,
    `names: ${list(cfg.names)}`,
    `descend: ${list(cfg.descend)}`,
    `prune: ${list(cfg.prune)}`,
    `gitignore: ${cfg.noIgnore ? 'not read' : 'respected'}`,
  ].join(' · ');
}

async function confirm(uris: vscode.Uri[]): Promise<boolean> {
  const listed = uris.slice(0, MAX_LISTED).map(uri => vscode.workspace.asRelativePath(uri, true));
  const hidden = uris.length - listed.length;
  const detail = hidden > 0 ? [...listed, `…and ${hidden} more`].join('\n') : listed.join('\n');

  const choice = await vscode.window.showWarningMessage(
    `Remove ${directories(uris.length)}?`,
    { modal: true, detail },
    'Remove',
  );
  return choice === 'Remove';
}

function report(result: RunResult, dryRun: boolean): void {
  if (revealLog()) log.show(true);

  if (result.uris.length === 0 && result.errors === 0) {
    vscode.window.showInformationMessage(
      result.noTargets
        ? 'No directory names are configured — set "rmccwrites.names".'
        : 'No empty directories found.',
    );
    return;
  }

  const what = `${directories(result.removed)} ${dryRun ? 'removable' : 'removed'}`;
  if (result.errors > 0) {
    void vscode.window
      .showWarningMessage(`${what}, ${result.errors === 1 ? '1 error' : `${result.errors} errors`}.`, 'Show Log')
      .then(choice => {
        if (choice === 'Show Log') log.show(true);
      });
    return;
  }
  void vscode.window.showInformationMessage(`${what}.`, 'Show Log').then(choice => {
    if (choice === 'Show Log') log.show(true);
  });
}

function workspaceRoots(): vscode.Uri[] {
  const folders = vscode.workspace.workspaceFolders;
  if (!folders || folders.length === 0) {
    vscode.window.showErrorMessage('Remove .cc-writes: Open a folder or workspace first.');
    return [];
  }
  return folders.map(folder => folder.uri);
}

/** The explorer selection, which the click target alone stands in for. */
function selectedRoots(uri: vscode.Uri | undefined, uris: vscode.Uri[] | undefined): vscode.Uri[] {
  if (uris && uris.length > 0) return uris;
  return uri ? [uri] : workspaceRoots();
}

function settingsFor(scope: vscode.Uri): RawSettings {
  const cfg = vscode.workspace.getConfiguration('rmccwrites', scope);
  return {
    names: cfg.get('names'),
    descend: cfg.get('descend'),
    prune: cfg.get('prune'),
    respectGitignore: cfg.get('respectGitignore'),
  };
}

function confirmBeforeRemoving(): boolean {
  return vscode.workspace.getConfiguration('rmccwrites').get('confirmBeforeRemoving', true);
}

function revealLog(): boolean {
  return vscode.workspace.getConfiguration('rmccwrites').get('revealLog', false);
}
