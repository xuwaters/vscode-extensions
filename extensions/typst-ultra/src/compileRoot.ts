import * as path from 'path';
import * as vscode from 'vscode';
import type { Client } from './lsp/client.js';
import * as config from './config.js';
import { rankEntryCandidates } from './entryPoints.js';

/**
 * Whether the compile root has been decided, as everything that has to respect
 * one sees it. Implemented by `CompileRoot`; an interface so the preview and
 * the export command depend on the question rather than on the status bar item.
 */
export interface RootState {
  /**
   * The file the server compiles, when that has been settled for it — a
   * session pin or `typstUltra.mainFile`. `undefined` means the root is
   * whatever the reader is looking at.
   *
   * Deliberately not just the pin: a project that checks its entry point into
   * `.vscode/settings.json` has answered the same question, and a caller that
   * only asked about the pin would compile the wrong file for it.
   */
  readonly entry: vscode.Uri | undefined;
}

/** What the preview needs on top of `RootState`: a way to *ask* about a root. */
export interface RootAdvisor extends RootState {
  suggestEntry(blank: vscode.Uri): Promise<void>;
}

/** Which of the three inputs decided the compile root. */
export type RootMode =
  | { kind: 'pinned'; uri: vscode.Uri }
  | { kind: 'setting'; uri: vscode.Uri }
  | { kind: 'following'; uri: vscode.Uri }
  | { kind: 'none' };

const PIN_KEY = 'typstUltra.mainFile';
const SUGGESTED_KEY = 'typstUltra.mainSuggested';

/**
 * The compile root, in all three of its modes.
 *
 * Following the focused editor is the zero-configuration default, and it is
 * right for single-file documents and for reading someone else's project. A
 * settled main file is right for a project of several files, where editing
 * `chapters/03.typ` or `data.typ` should still produce whole-document
 * diagnostics — and, since the preview follows this too, a whole document to
 * look at rather than the blank page a file of `#let` bindings compiles to.
 *
 * The entry file is the first of:
 *
 * 1. the session pin (`typstUltra.pinMain`, stored in `workspaceState`),
 * 2. `typstUltra.mainFile`, which a team can check in,
 * 3. the focused `.typ` editor.
 */
export class CompileRoot implements vscode.Disposable, RootAdvisor {
  private readonly item: vscode.StatusBarItem;
  private readonly disposables: vscode.Disposable[] = [];
  private mode: RootMode = { kind: 'none' };

  constructor(
    private readonly context: vscode.ExtensionContext,
    private readonly client: Client,
  ) {
    this.item = vscode.window.createStatusBarItem(
      vscode.StatusBarAlignment.Right,
      100,
    );
    this.item.command = 'typstUltra.selectMain';
    this.disposables.push(this.item);

    this.disposables.push(
      vscode.commands.registerCommand('typstUltra.selectMain', () => this.pick()),
      vscode.window.onDidChangeActiveTextEditor((editor) => this.follow(editor)),
      // `typstUltra.mainFile` can change without the focus moving — someone
      // edits `.vscode/settings.json`, or pulls a branch that adds it — and
      // the status bar has to say so at once, not at the next click.
      vscode.workspace.onDidChangeConfiguration((event) => {
        if (event.affectsConfiguration('typstUltra.mainFile')) {
          this.follow(vscode.window.activeTextEditor);
        }
      }),
    );

    this.follow(vscode.window.activeTextEditor);
  }

  /** The current mode, for the status bar and tests. */
  get current(): RootMode {
    return this.mode;
  }

  /** Whether a main file is pinned for this session. */
  get pinned(): vscode.Uri | undefined {
    const stored = this.context.workspaceState.get<string>(PIN_KEY);
    return stored ? vscode.Uri.file(stored) : undefined;
  }

  /**
   * The settled compile root: the session pin, else `typstUltra.mainFile`.
   *
   * Reads its inputs afresh rather than the mode `follow` last computed. A
   * setting can change without the focus moving, and a preview that acted on a
   * stale answer would go on compiling the wrong file until the reader
   * happened to click something.
   */
  get entry(): vscode.Uri | undefined {
    return (
      this.pinned ??
      this.configured(vscode.window.activeTextEditor?.document.uri)
    );
  }

  /** Pin a file as the compile root. */
  async pin(uri: vscode.Uri): Promise<void> {
    await this.context.workspaceState.update(PIN_KEY, uri.fsPath);
    this.client.notify('typst/setMain', { uri: uri.toString() });
    this.refresh();
  }

  /** Go back to following the focused editor. */
  async unpin(): Promise<void> {
    await this.context.workspaceState.update(PIN_KEY, undefined);
    this.client.notify('typst/setMain', { uri: null });
    this.follow(vscode.window.activeTextEditor);
  }

  /** Update after the active editor changed. */
  follow(editor: vscode.TextEditor | undefined): void {
    const pinned = this.pinned;
    if (pinned) {
      this.mode = { kind: 'pinned', uri: pinned };
      this.refresh();
      return;
    }

    const configured = this.configured(editor?.document.uri);
    if (configured) {
      this.mode = { kind: 'setting', uri: configured };
      this.refresh();
      return;
    }

    if (editor?.document.languageId === 'typst') {
      this.mode = { kind: 'following', uri: editor.document.uri };
      void this.suggestPinning(editor.document.uri);
    } else if (this.mode.kind !== 'following') {
      this.mode = { kind: 'none' };
    }
    this.refresh();
  }

  /** Show the status bar item, or hide it when there is nothing to say. */
  refresh(): void {
    switch (this.mode.kind) {
      case 'pinned':
        this.item.text = `$(pin) ${path.basename(this.mode.uri.fsPath)}`;
        this.item.tooltip = `Typst: compiling the pinned main file. Click to change.`;
        this.item.show();
        break;
      case 'setting':
        this.item.text = `$(settings) ${path.basename(this.mode.uri.fsPath)}`;
        this.item.tooltip =
          'Typst: compiling the file from `typstUltra.mainFile`. Click to change.';
        this.item.show();
        break;
      case 'following':
        this.item.text = `$(eye) ${path.basename(this.mode.uri.fsPath)}`;
        this.item.tooltip = 'Typst: following the focused editor. Click to pin.';
        this.item.show();
        break;
      case 'none':
        this.item.hide();
        break;
    }

    void vscode.commands.executeCommand(
      'setContext',
      'typstUltra.mainPinned',
      this.mode.kind === 'pinned',
    );
  }

  /** The QuickPick behind the status bar item. */
  async pick(): Promise<void> {
    const active = vscode.window.activeTextEditor?.document.uri;
    const candidates = await this.candidates();

    interface Item extends vscode.QuickPickItem {
      action: 'unpin' | 'pin';
      uri?: vscode.Uri;
    }

    const items: Item[] = [];
    if (this.mode.kind === 'pinned') {
      items.push({
        label: '$(eye) Follow the focused editor',
        description: 'Unpin the compile root',
        action: 'unpin',
      });
    }
    if (active) {
      items.push({
        label: `$(pin) Pin ${path.basename(active.fsPath)}`,
        description: 'The file you are looking at',
        action: 'pin',
        uri: active,
      });
    }
    for (const uri of candidates) {
      if (active && uri.fsPath === active.fsPath) continue;
      items.push({
        label: `$(file) ${vscode.workspace.asRelativePath(uri)}`,
        action: 'pin',
        uri,
      });
    }

    const choice = await vscode.window.showQuickPick(items, {
      title: 'Typst: compile root',
      placeHolder: 'Which file should be compiled?',
    });
    if (!choice) return;

    if (choice.action === 'unpin') {
      await this.unpin();
    } else if (choice.uri) {
      await this.pin(choice.uri);
    }
  }

  /**
   * Offer to settle the compile root after a document compiled to nothing.
   *
   * A file of `#let` bindings or a template of `#show` rules is a perfectly
   * good `.typ` that produces no pages, and a preview of one is a blank tab
   * with no explanation. Where the `main.typ` offer below guesses from the file
   * *names* in the project, this fires on the symptom itself, so it reaches the
   * layouts no naming convention would have caught.
   *
   * Shares the one-shot counter with that offer: between the two of them a
   * reader should be asked about the compile root once.
   */
  async suggestEntry(blank: vscode.Uri): Promise<void> {
    if (this.entry) return;
    if (this.context.workspaceState.get<boolean>(SUGGESTED_KEY)) return;

    const candidates = await this.candidates(blank);
    const best = candidates[0];
    if (!best) return;

    await this.context.workspaceState.update(SUGGESTED_KEY, true);

    const choice = await vscode.window.showInformationMessage(
      `Typst: ${path.basename(blank.fsPath)} compiles to no pages, so it looks like part of a larger document rather than one of its own. Preview ${vscode.workspace.asRelativePath(best)} instead?`,
      `Pin ${path.basename(best.fsPath)}`,
      'Choose a file…',
    );
    if (choice === 'Choose a file…') {
      await this.pick();
    } else if (choice) {
      await this.pin(best);
    }
  }

  dispose(): void {
    for (const disposable of this.disposables) disposable.dispose();
  }

  /**
   * Every `.typ` in the workspace, likeliest entry point first, optionally
   * without one file — the one that just failed to be a document.
   */
  private async candidates(exclude?: vscode.Uri): Promise<vscode.Uri[]> {
    const found = await vscode.workspace.findFiles(
      '**/*.typ',
      '**/node_modules/**',
      200,
    );
    const skip = exclude?.toString();
    const byPath = new Map<string, vscode.Uri>();
    for (const uri of found) {
      if (uri.toString() === skip) continue;
      byPath.set(vscode.workspace.asRelativePath(uri), uri);
    }
    return rankEntryCandidates([...byPath.keys()]).map(
      (relative) => byPath.get(relative) as vscode.Uri,
    );
  }

  /** `typstUltra.mainFile`, resolved against the project root. */
  private configured(scope: vscode.Uri | undefined): vscode.Uri | undefined {
    const settings = config.read(scope);
    if (!settings.host.mainFile) return undefined;
    const root = config.resolveRoot(settings, scope);
    const absolute = path.isAbsolute(settings.host.mainFile)
      ? settings.host.mainFile
      : path.join(root, settings.host.mainFile);
    return vscode.Uri.file(absolute);
  }

  /**
   * Offer to pin `main.typ` once, the first time someone opens a chapter in a
   * project that clearly has one.
   *
   * One-shot per workspace: an offer that comes back is nagging, and the
   * command and the status bar are both still there for anyone who declines.
   */
  private async suggestPinning(current: vscode.Uri): Promise<void> {
    if (this.context.workspaceState.get<boolean>(SUGGESTED_KEY)) return;

    const folder = vscode.workspace.getWorkspaceFolder(current);
    if (!folder) return;

    const main = vscode.Uri.joinPath(folder.uri, 'main.typ');
    if (main.fsPath === current.fsPath) return;
    try {
      await vscode.workspace.fs.stat(main);
    } catch {
      return;
    }

    await this.context.workspaceState.update(SUGGESTED_KEY, true);

    const choice = await vscode.window.showInformationMessage(
      'This project has a `main.typ`. Pin it as the compile root so editing a chapter still checks the whole document?',
      'Pin main.typ',
      'Not now',
    );
    if (choice === 'Pin main.typ') await this.pin(main);
  }
}
