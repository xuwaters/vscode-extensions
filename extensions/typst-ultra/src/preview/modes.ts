import * as vscode from 'vscode';
import {
  activePreviewEditorUri,
  isPreviewable,
  openPreviewEditor,
  openSource,
} from './editors.js';
import type { PreviewManager } from './manager.js';
import { CYCLE, resolveMode, toggleEditPreview, type PreviewMode } from './modeState.js';

export type { PreviewMode };

const MODE_LABEL: Record<PreviewMode, string> = {
  edit: '$(edit) Edit',
  // Not `$(split-horizontal)`: that is the icon VSCode's own Split Editor Right
  // button uses, and the two sit side by side in the editor title bar.
  split: '$(layout-sidebar-right) Split',
  preview: '$(preview) Preview',
};

const CTX_MODE = 'typstUltra.mode';

/**
 * Owns the view-mode state machine and its status-bar switcher.
 *
 * Transitions reuse what is already on screen: with a panel open,
 * `revealPanel` moves it between columns without reloading the webview; with
 * only the source open, Edit ↔ Preview swaps the editor *inside the same tab*,
 * so toggling the view never grows the tab bar.
 *
 * The same three modes markdown-preview-ultra has, on purpose — down to the
 * keybindings and the icons. A reader who has learned `cmd+shift+v` in one
 * should not have to learn a second meaning for it in the other.
 */
export class ModeManager implements vscode.Disposable {
  private readonly statusBar: vscode.StatusBarItem;
  private readonly disposables: vscode.Disposable[] = [];

  constructor(private readonly manager: PreviewManager) {
    this.statusBar = vscode.window.createStatusBarItem(
      'typstUltra.mode',
      vscode.StatusBarAlignment.Right,
      97,
    );
    this.statusBar.name = 'Typst Ultra Mode';
    this.statusBar.command = 'typstUltra.switchMode';
    this.disposables.push(
      this.statusBar,
      manager.onDidChangeState(() => this.refresh()),
      vscode.window.onDidChangeActiveTextEditor(() => this.refresh()),
      // A preview-editor tab is not a text editor, so activating one is only
      // visible as a tab change.
      vscode.window.tabGroups.onDidChangeTabs(() => this.refresh()),
      vscode.window.tabGroups.onDidChangeTabGroups(() => this.refresh()),
    );
    this.refresh();
  }

  currentMode(): PreviewMode {
    return resolveMode({
      previewEditorActive: activePreviewEditorUri() !== undefined,
      hasPanel: this.manager.hasPreview,
      panelColumn: this.manager.panelColumn,
      sourceColumn: this.manager.sourceColumn,
    });
  }

  async cycleMode(): Promise<void> {
    await this.setMode(CYCLE[this.currentMode()]);
  }

  /** One key for both directions: Edit → Preview, Preview → Edit. */
  async toggleEditPreview(): Promise<void> {
    await this.setMode(toggleEditPreview(this.currentMode()));
  }

  async switchMode(): Promise<void> {
    const current = this.currentMode();
    const picked = await vscode.window.showQuickPick(
      (['edit', 'split', 'preview'] as const).map((mode) => ({
        label: MODE_LABEL[mode],
        description: mode === current ? 'current' : undefined,
        mode,
      })),
      { title: 'Typst Ultra', placeHolder: 'Preview mode' },
    );
    if (picked) await this.setMode(picked.mode);
  }

  async setMode(target: PreviewMode): Promise<void> {
    const current = this.currentMode();
    if (target === current) {
      this.refresh();
      return;
    }

    const previewEditor = activePreviewEditorUri();
    if (previewEditor) {
      await this.leavePreviewEditor(previewEditor, target);
      this.refresh();
      return;
    }

    const uri = this.manager.currentUri;
    if (!uri) return this.noDocument();

    switch (target) {
      case 'edit': {
        // Read the layout before closing anything: the manager forgets where
        // the source lives the moment its panel goes away.
        const existing = sourceEditorOf(uri)?.viewColumn;
        const sourceColumn = this.manager.sourceColumn;
        this.manager.closePreview();
        const document = await vscode.workspace.openTextDocument(uri);
        // From Split the source editor is already open beside the panel;
        // reveal *that* one. Without an explicit column `showTextDocument`
        // targets the active column — the panel's — and opens a second copy of
        // the document alongside the original.
        await vscode.window.showTextDocument(document, {
          viewColumn: existing ?? sourceColumn ?? vscode.ViewColumn.One,
          preserveFocus: false,
        });
        break;
      }

      case 'split': {
        if (this.manager.hasPreview) {
          // Preview → Split: move the panel out of the source column, locking
          // the group it lands in before taking focus off it — the lock is
          // applied to whichever group is active when it runs.
          await this.manager.revealPanel(vscode.ViewColumn.Beside, false);
          const document = await vscode.workspace.openTextDocument(uri);
          await vscode.window.showTextDocument(document, {
            viewColumn: this.manager.sourceColumn,
            preserveFocus: false,
          });
        } else {
          await this.manager.show(uri, vscode.ViewColumn.Beside);
        }
        break;
      }

      case 'preview': {
        if (!isPreviewable(uri)) {
          // A `.typc` or an untitled buffer: the preview editor is bound to the
          // file patterns it declares and cannot open this, so the panel takes
          // the column instead.
          await this.manager.show(uri, vscode.ViewColumn.Active);
          break;
        }
        // Preview mode *is* the preview editor: the tab holding the source
        // becomes the preview, rather than gaining a second tab in front of it.
        const column =
          sourceEditorOf(uri)?.viewColumn ??
          this.manager.sourceColumn ??
          vscode.window.tabGroups.activeTabGroup.viewColumn ??
          vscode.ViewColumn.One;
        this.manager.closePreview();
        await openPreviewEditor(uri, column);
        break;
      }
    }

    this.refresh();
  }

  /**
   * Hand a preview-editor tab back to the text editor. The tab belongs to
   * VSCode and shows a single file for its life, so both remaining modes go
   * through the source: Edit replaces the tab with it, Split does that and then
   * opens the following panel beside it.
   */
  private async leavePreviewEditor(
    uri: vscode.Uri,
    target: PreviewMode,
  ): Promise<void> {
    if (target === 'preview') return;
    const column =
      vscode.window.tabGroups.activeTabGroup.viewColumn ?? vscode.ViewColumn.One;
    // The source takes over the preview's tab rather than opening in front of
    // it, so the tab bar is the same width either side of the switch.
    await openSource(uri, column);
    if (target === 'edit') return;
    await this.manager.show(uri, vscode.ViewColumn.Beside);
  }

  private noDocument(): void {
    void vscode.window.showInformationMessage(
      'Typst: open a .typ file to show its preview.',
    );
  }

  private refresh(): void {
    const mode = this.currentMode();
    void vscode.commands.executeCommand('setContext', CTX_MODE, mode);
    this.statusBar.text = MODE_LABEL[mode];
    this.statusBar.tooltip = 'Switch the Typst preview mode';

    const editor = vscode.window.activeTextEditor;
    const relevant =
      this.manager.hasPreview ||
      editor?.document.languageId === 'typst' ||
      activePreviewEditorUri() !== undefined;
    if (relevant) {
      this.statusBar.show();
    } else {
      this.statusBar.hide();
    }
  }

  dispose(): void {
    for (const disposable of this.disposables) disposable.dispose();
  }
}

/**
 * An on-screen text editor for `uri`. The active one wins over any other copy
 * of the same file, so the tab the reader is looking at is the tab a mode
 * switch acts on.
 */
function sourceEditorOf(uri: vscode.Uri): vscode.TextEditor | undefined {
  const key = uri.toString();
  const active = vscode.window.activeTextEditor;
  if (active?.document.uri.toString() === key) return active;
  return vscode.window.visibleTextEditors.find(
    (editor) => editor.document.uri.toString() === key,
  );
}
