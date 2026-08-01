import * as vscode from 'vscode';
import { PreviewManager, canPreview } from './previewManager';

/**
 * The three view modes. The mode is *derived* from observable panel/editor
 * state (not shadow state), so it can never fight VSCode's own layout
 * persistence:
 *
 * - `edit`    — no preview panel.
 * - `preview` — the panel occupies the source editor's column.
 * - `split`   — the panel lives in another column.
 */
export type PreviewMode = 'edit' | 'split' | 'preview';

const CYCLE: Record<PreviewMode, PreviewMode> = {
  edit: 'split',
  split: 'preview',
  preview: 'edit',
};

const MODE_LABEL: Record<PreviewMode, string> = {
  edit: '$(edit) Edit',
  split: '$(split-horizontal) Split',
  preview: '$(eye) Preview',
};

const CTX_MODE = 'markdownPreviewUltra.mode';

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
    );
    this.refresh();
  }

  public currentMode(): PreviewMode {
    if (!this.manager.hasPreview) return 'edit';
    const panelColumn = this.manager.panelColumn;
    const sourceColumn = this.manager.sourceColumn;
    return panelColumn !== undefined && panelColumn === sourceColumn
      ? 'preview'
      : 'split';
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
    const document = this.manager.currentDocument;

    switch (target) {
      case 'edit': {
        this.manager.closePreview();
        if (document) {
          await vscode.window.showTextDocument(document, {
            preserveFocus: false,
          });
        }
        break;
      }
      case 'split': {
        if (!document) return this.noDocument();
        if (this.manager.hasPreview) {
          // Preview → Split: move the panel out of the source column.
          this.manager.revealPanel(vscode.ViewColumn.Beside, false);
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
          this.manager.revealPanel(this.manager.sourceColumn, false);
        } else {
          this.manager.showPreview(document, vscode.ViewColumn.Active);
        }
        break;
      }
    }
    this.refresh();
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
      this.manager.hasPreview || canPreview(editor?.document);
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
