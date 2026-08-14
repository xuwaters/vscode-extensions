import * as vscode from 'vscode';
import { openPreviewEditor, tabResource } from './customEditor';
import { shouldPromoteToPreview } from './modeState';
import type { PreviewManager } from './previewManager';
import { isMarkdownDocument, isPreviewEditorPath } from './util';

/**
 * Hands markdown files over to the preview as they are opened — and knows when
 * not to.
 *
 * This extension used to do the same job with a `workbench.editorAssociations`
 * default, which had the merit of opening the preview with no flash of source
 * at all. It also opened it for *every* way of reaching a markdown file, and
 * one of those ways carries a destination: a search result names the line and
 * column to reveal, and VSCode drops both when the editor it resolves to is a
 * custom editor. A reader who clicked a result landed on a rendered page that
 * could not tell them where their keyword was.
 *
 * So the text editor opens first and is handed over a moment later, once there
 * is something to read the reader's intent off — see `shouldPromoteToPreview`.
 * The cost is the flash of source the association avoided; the association is
 * still there for anyone who would rather have it back (`customEditor.ts`
 * recovers the match separately for them).
 */
export class PreviewPromoter implements vscode.Disposable {
  /**
   * Files whose placement is already decided, so that coming back to a tab is
   * not a second chance to move it: a reader who is on the source of a file
   * stays there, whatever put them there. An entry is dropped when the file's
   * last tab closes, so opening it again decides afresh.
   */
  private readonly settled = new Set<string>();
  private readonly disposables: vscode.Disposable[] = [];

  constructor(private readonly manager: PreviewManager) {
    // Tabs restored from the last window stand where the reader left them.
    for (const group of vscode.window.tabGroups.all) {
      for (const tab of group.tabs) {
        const uri = tabResource(tab);
        if (uri) this.settled.add(uri.toString());
      }
    }
    this.disposables.push(
      vscode.window.onDidChangeActiveTextEditor((editor) => {
        void this.onActiveEditor(editor);
      }),
      vscode.window.tabGroups.onDidChangeTabs((e) => this.forget(e.closed)),
    );
  }

  /**
   * Record that this extension put `uri` where it is. Every deliberate move
   * between the source and the preview goes through the text editor at some
   * point — switching to Edit mode most obviously — and without this the
   * promoter would read that as a file arriving and send it straight back.
   */
  public settle(uri: vscode.Uri): void {
    this.settled.add(uri.toString());
  }

  public dispose(): void {
    for (const d of this.disposables) d.dispose();
  }

  private async onActiveEditor(
    editor: vscode.TextEditor | undefined,
  ): Promise<void> {
    if (!editor || editor.viewColumn === undefined) return;
    const { document } = editor;
    if (!isMarkdownDocument(document)) return;
    // Markdown by language but not by name — an untitled buffer, say. The
    // preview editor is bound to the filenames it declares and cannot take it.
    if (!isPreviewEditorPath(document.uri.fsPath)) return;
    const key = document.uri.toString();
    if (this.settled.has(key)) return;
    // Decided either way, and before the await: leaving a file on the source is
    // as much a decision as moving it, and the hand-over below makes this
    // editor active a second time on its way out.
    this.settled.add(key);
    if (
      !shouldPromoteToPreview(
        {
          hasPanel: this.manager.hasPreview,
          panelColumn: this.manager.panelColumn,
          sourceColumn: this.manager.sourceColumn,
        },
        { column: editor.viewColumn, hasSelection: !editor.selection.isEmpty },
        previewByDefault(),
      )
    ) {
      return;
    }
    try {
      // In place: this tab is the one the reader is looking at, and whatever
      // opened it settled whether that came with focus. Only what is inside it
      // changes.
      await openPreviewEditor(document.uri, editor.viewColumn, true);
    } catch (err) {
      // The file stays on the source, which is a worse view of it than the
      // reader asked for but a working one.
      console.error('markdown-preview-ultra: opening the preview failed', err);
    }
  }

  /**
   * Drop the files whose last tab has just closed. A file still showing
   * somewhere else — the source column of a split, another group — is still
   * placed, so only the closing of the last of its tabs frees it.
   */
  private forget(closed: readonly vscode.Tab[]): void {
    if (closed.length === 0) return;
    const open = new Set<string>();
    for (const group of vscode.window.tabGroups.all) {
      for (const tab of group.tabs) {
        const uri = tabResource(tab);
        if (uri) open.add(uri.toString());
      }
    }
    for (const tab of closed) {
      const key = tabResource(tab)?.toString();
      if (key !== undefined && !open.has(key)) this.settled.delete(key);
    }
  }
}

/** Whether opening a markdown file should land in the preview. */
function previewByDefault(): boolean {
  return vscode.workspace
    .getConfiguration('markdownPreviewUltra')
    .get<boolean>('openInPreview', true);
}
