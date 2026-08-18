import * as path from 'path';
import * as vscode from 'vscode';
import type { Client } from './client.js';
import * as config from './config.js';

/** Which of the three inputs decided the compile root. */
export type RootMode =
  | { kind: 'pinned'; uri: vscode.Uri }
  | { kind: 'setting'; uri: vscode.Uri }
  | { kind: 'following'; uri: vscode.Uri }
  | { kind: 'none' };

const PIN_KEY = 'typstUltra.mainFile';
const SUGGESTED_KEY = 'typstUltra.mainSuggested';

/**
 * The compile root, in both of its modes.
 *
 * Following the focused editor is the zero-configuration default, and it is
 * right for single-file documents and for reading someone else's project. A
 * pinned main file is right for a book with chapters, where editing
 * `chapters/03.typ` should still produce whole-document diagnostics.
 *
 * The entry file is the first of:
 *
 * 1. the session pin (`typstUltra.pinMain`, stored in `workspaceState`),
 * 2. `typstUltra.mainFile`, which a team can check in,
 * 3. the focused `.typ` editor.
 */
export class CompileRoot implements vscode.Disposable {
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
    );

    this.follow(vscode.window.activeTextEditor);
  }

  /** The current mode, for the status bar and tests. */
  get current(): RootMode {
    return this.mode;
  }

  /** Whether a main file is pinned. */
  get pinned(): vscode.Uri | undefined {
    const stored = this.context.workspaceState.get<string>(PIN_KEY);
    return stored ? vscode.Uri.file(stored) : undefined;
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

    const settings = config.read(editor?.document.uri);
    if (settings.host.mainFile) {
      const root = config.resolveRoot(settings, editor?.document.uri);
      const absolute = path.isAbsolute(settings.host.mainFile)
        ? settings.host.mainFile
        : path.join(root, settings.host.mainFile);
      this.mode = { kind: 'setting', uri: vscode.Uri.file(absolute) };
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
    const candidates = await vscode.workspace.findFiles('**/*.typ', '**/node_modules/**', 200);

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

  dispose(): void {
    for (const disposable of this.disposables) disposable.dispose();
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
