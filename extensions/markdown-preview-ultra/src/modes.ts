import * as vscode from 'vscode';
import { activePreviewEditorUri, openSource } from './customEditor';
import { resolveMode, type PreviewMode } from './modeState';
import { PreviewManager, canPreview } from './previewManager';

export type { PreviewMode };

const CYCLE: Record<PreviewMode, PreviewMode> = {
  edit: 'split',
  split: 'preview',
  preview: 'edit',
};

const MODE_LABEL: Record<PreviewMode, string> = {
  edit: '$(edit) Edit',
  split: '$(split-horizontal) Split',
  preview: '$(preview) Preview',
};

const CTX_MODE = 'markdownPreviewUltra.mode';

/** Column of an already-open editor for `document`, if one is on screen. */
function visibleColumnOf(
  document: vscode.TextDocument,
): vscode.ViewColumn | undefined {
  const uri = document.uri.toString();
  return vscode.window.visibleTextEditors.find(
    (editor) => editor.document.uri.toString() === uri,
  )?.viewColumn;
}

/**
 * Owns the view-mode state machine and its status-bar switcher. Transitions
 * reuse the existing panel where possible (`panel.reveal` moves it between
 * columns without reloading the webview).
 */
export class ModeManager implements vscode.Disposable {
  private readonly statusBar: vscode.StatusBarItem;
  private readonly disposables: vscode.Disposable[] = [];

  constructor(private readonly manager: PreviewManager) {
    this.statusBar = vscode.window.createStatusBarItem(
      'markdownPreviewUltra.mode',
      vscode.StatusBarAlignment.Right,
      100,
    );
    this.statusBar.name = 'Markdown Preview Ultra Mode';
    this.statusBar.command = 'markdownPreviewUltra.switchMode';
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

  public currentMode(): PreviewMode {
    return resolveMode({
      previewEditorActive: activePreviewEditorUri() !== undefined,
      hasPanel: this.manager.hasPreview,
      panelColumn: this.manager.panelColumn,
      sourceColumn: this.manager.sourceColumn,
    });
  }

  public async cycleMode(): Promise<void> {
    await this.setMode(CYCLE[this.currentMode()]);
  }

  public async switchMode(): Promise<void> {
    const current = this.currentMode();
    const picked = await vscode.window.showQuickPick(
      (['edit', 'split', 'preview'] as const).map((mode) => ({
        label: MODE_LABEL[mode],
        description: mode === current ? 'current' : undefined,
        mode,
      })),
      { placeHolder: 'Markdown preview mode' },
    );
    if (picked) await this.setMode(picked.mode);
  }

  public async setMode(target: PreviewMode): Promise<void> {
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
    const document = this.manager.currentDocument;

    switch (target) {
      case 'edit': {
        // Capture the columns *before* disposing the panel — closing it clears
        // the manager's record of where the source editor lives.
        const existingColumn = document ? visibleColumnOf(document) : undefined;
        const sourceColumn = this.manager.sourceColumn;
        const readTo = this.manager.closePreview();
        if (document) {
          // From split the source editor is already open beside the panel;
          // reveal *that* one. Without an explicit column `showTextDocument`
          // targets the active column — the panel's — and opens a second copy
          // of the document alongside the original.
          const editor = await vscode.window.showTextDocument(document, {
            viewColumn:
              existingColumn ?? sourceColumn ?? vscode.ViewColumn.One,
            preserveFocus: false,
          });
          // Leaving Preview mode should land on the passage being read, not
          // wherever the editor was parked before the preview took the column.
          if (readTo !== undefined) {
            editor.revealRange(
              new vscode.Range(readTo, 0, readTo, 0),
              vscode.TextEditorRevealType.AtTop,
            );
          }
        }
        break;
      }
      case 'split': {
        if (!document) return this.noDocument();
        if (this.manager.hasPreview) {
          // Preview → Split: move the panel out of the source column, locking
          // the group it lands in before taking focus off it — the lock is
          // applied to whichever group is active when it runs.
          await this.manager.revealPanel(vscode.ViewColumn.Beside, false);
          await vscode.window.showTextDocument(document, {
            viewColumn: this.manager.sourceColumn,
            preserveFocus: false,
          });
        } else {
          this.manager.showPreview(document, vscode.ViewColumn.Beside);
        }
        break;
      }
      case 'preview': {
        if (!document) return this.noDocument();
        if (this.manager.hasPreview && this.manager.sourceColumn !== undefined) {
          // Split → Preview: move the panel into the source column.
          await this.manager.revealPanel(this.manager.sourceColumn, false);
        } else {
          this.manager.showPreview(document, vscode.ViewColumn.Active);
        }
        break;
      }
    }
    this.refresh();
  }

  /**
   * Hand a preview-editor tab back to the text editor. The tab belongs to
   * VSCode and shows a single file for its life, so both remaining modes go
   * through the source: Edit replaces the tab with it, Split does that and
   * then opens the following panel beside it.
   */
  private async leavePreviewEditor(
    uri: vscode.Uri,
    target: PreviewMode,
  ): Promise<void> {
    if (target === 'preview') return;
    const column =
      vscode.window.tabGroups.activeTabGroup.viewColumn ??
      vscode.ViewColumn.One;
    // Reopening the same resource in the same group swaps the editor for it,
    // so the preview tab is replaced rather than added to.
    await openSource(uri, column);
    if (target === 'edit') return;
    const document = await vscode.workspace.openTextDocument(uri);
    this.manager.showPreview(document, vscode.ViewColumn.Beside);
  }

  private noDocument(): void {
    vscode.window.showInformationMessage(
      'Open a Markdown file to show its preview.',
    );
  }

  private refresh(): void {
    const mode = this.currentMode();
    void vscode.commands.executeCommand('setContext', CTX_MODE, mode);
    this.statusBar.text = MODE_LABEL[mode];
    this.statusBar.tooltip = 'Switch markdown preview mode';

    const editor = vscode.window.activeTextEditor;
    const relevant =
      this.manager.hasPreview ||
      canPreview(editor?.document) ||
      activePreviewEditorUri() !== undefined;
    if (relevant) {
      this.statusBar.show();
    } else {
      this.statusBar.hide();
    }
  }

  public dispose(): void {
    for (const d of this.disposables) d.dispose();
  }
}
