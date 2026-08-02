import * as vscode from 'vscode';
import {
  activePreviewEditorUri,
  isPreviewable,
  MarkdownEditorProvider,
  openPreviewEditor,
  openSource,
} from './customEditor';
import { resolveMode, toggleEditPreview, type PreviewMode } from './modeState';
import { PreviewManager, canPreview } from './previewManager';

export type { PreviewMode };

const CYCLE: Record<PreviewMode, PreviewMode> = {
  edit: 'split',
  split: 'preview',
  preview: 'edit',
};

const MODE_LABEL: Record<PreviewMode, string> = {
  edit: '$(edit) Edit',
  // Not `$(split-horizontal)`: that is the icon VSCode's own Split Editor Right
  // button uses, and the two sit side by side in the editor title bar.
  split: '$(layout-sidebar-right) Split',
  preview: '$(preview) Preview',
};

const CTX_MODE = 'markdownPreviewUltra.mode';

/**
 * An on-screen text editor for `document`. The active one wins over any other
 * copy of the same file, so the tab the reader is looking at is the tab a mode
 * switch acts on.
 */
function sourceEditorOf(
  document: vscode.TextDocument,
): vscode.TextEditor | undefined {
  const uri = document.uri.toString();
  const active = vscode.window.activeTextEditor;
  if (active?.document.uri.toString() === uri) return active;
  return vscode.window.visibleTextEditors.find(
    (editor) => editor.document.uri.toString() === uri,
  );
}

/**
 * Owns the view-mode state machine and its status-bar switcher.
 *
 * Transitions reuse what is already on screen: with a panel open, `panel.reveal`
 * moves it between columns without reloading the webview; with only the source
 * open, Edit ↔ Preview swaps the editor *inside the same tab*, so toggling the
 * view never grows the tab bar.
 */
export class ModeManager implements vscode.Disposable {
  private readonly statusBar: vscode.StatusBarItem;
  private readonly disposables: vscode.Disposable[] = [];

  constructor(
    private readonly manager: PreviewManager,
    private readonly editors: MarkdownEditorProvider,
  ) {
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

  /** One key for both directions: Edit → Preview, Preview → Edit. */
  public async toggleEditPreview(): Promise<void> {
    await this.setMode(toggleEditPreview(this.currentMode()));
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
        const existingColumn = document
          ? sourceEditorOf(document)?.viewColumn
          : undefined;
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
        if (!isPreviewable(document.uri)) {
          // Markdown by language, not by name (an untitled buffer, say). The
          // preview editor is bound to the file patterns it declares and cannot
          // open this, so the panel takes the column instead.
          this.manager.showPreview(document, vscode.ViewColumn.Active);
          break;
        }
        // Preview mode *is* the preview editor: the tab holding the source
        // becomes the preview, rather than gaining a second tab in front of it.
        // Read the layout before closing anything — the manager forgets where
        // the source lives the moment its panel goes away.
        const source = sourceEditorOf(document);
        const column =
          source?.viewColumn ??
          this.manager.sourceColumn ??
          vscode.window.tabGroups.activeTabGroup.viewColumn ??
          vscode.ViewColumn.One;
        // From Split, the panel hands back the line it was read to; from Edit,
        // the place to open at is wherever the editor is scrolled.
        const readTo = this.manager.closePreview();
        const line = readTo ?? source?.visibleRanges[0]?.start.line;
        if (line !== undefined) this.editors.parkLine(document.uri, line);
        await openPreviewEditor(document.uri, column);
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
    // The reader's place in the page, so the source opens on the passage they
    // were reading rather than wherever the editor was parked before.
    const line = this.editors.takeLine(uri);
    // The source takes over the preview's tab rather than opening in front of
    // it, so the tab bar is the same width either side of the switch.
    await openSource(uri, column, line === undefined ? undefined : { line });
    if (target === 'edit') return;
    const document = await vscode.workspace.openTextDocument(uri);
    // Both halves of the split open on the passage that was being read. The
    // panel is told the line rather than left to read it off the editor: the
    // reveal above has not been laid out yet, so the editor's visible range
    // still reports wherever it was parked before.
    this.manager.showPreview(document, vscode.ViewColumn.Beside, line);
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
