import * as vscode from 'vscode';

/**
 * Swapping a tab between the text editor and the preview editor.
 *
 * The point of all of this is that a mode switch must not grow the tab bar.
 * Opening a file's preview in front of its source leaves two tabs for one file,
 * and the reader has to close one of them; taking the tab over keeps its place
 * in the bar and hands unsaved changes across untouched.
 *
 * The mechanics are markdown-preview-ultra's, minus its search and editor-
 * association machinery: we never write `workbench.editorAssociations`
 * ([design/preview.md §1](../../../../docs/rfc/010-typst-ultra/design/preview.md)),
 * so nothing arrives in the preview editor except by being sent there.
 */

/** VSCode's built-in text editor, for handing a tab back to the source. */
const TEXT_EDITOR = 'default';

/** The preview editor's view type — the full-tab surface Preview mode uses. */
export const PREVIEW_EDITOR_VIEW_TYPE = 'typstUltra.editor';

/**
 * VSCode's own Reopen With, which swaps the editor *inside* the active tab.
 * `vscode.openWith` cannot: its resolver only reuses a tab when the editor type
 * matches, so opening a file's preview over its source leaves the source tab
 * sitting behind it.
 */
const REOPEN_ACTIVE_EDITOR_WITH = 'reopenActiveEditorWith';

/** What a tab is showing, for the two kinds of tab this extension opens. */
function tabEditor(
  tab: vscode.Tab,
): { uri: vscode.Uri; editorId: string } | undefined {
  const input = tab.input;
  if (input instanceof vscode.TabInputText) {
    return { uri: input.uri, editorId: TEXT_EDITOR };
  }
  if (input instanceof vscode.TabInputCustom) {
    return { uri: input.uri, editorId: input.viewType };
  }
  return undefined;
}

/**
 * The editor a tab in `column` is using to show `uri`, if one is. The active tab
 * wins, so a mode switch acts on the tab the reader is looking at.
 */
function editorShowing(
  uri: vscode.Uri,
  column: vscode.ViewColumn,
): string | undefined {
  const group = vscode.window.tabGroups.all.find(
    (candidate) => candidate.viewColumn === column,
  );
  if (!group) return undefined;
  const tabs = group.activeTab ? [group.activeTab, ...group.tabs] : group.tabs;
  for (const tab of tabs) {
    const shown = tabEditor(tab);
    if (shown?.uri.toString() === uri.toString()) return shown.editorId;
  }
  return undefined;
}

function openWith(
  uri: vscode.Uri,
  column: vscode.ViewColumn,
  editorId: string,
): Thenable<unknown> {
  return vscode.commands.executeCommand('vscode.openWith', uri, editorId, {
    viewColumn: column,
    preserveFocus: false,
  });
}

/**
 * Show `uri` in `column` under the editor `editorId`, taking over the tab that
 * already shows the file rather than opening in front of it.
 *
 * The symbolic columns are left to VSCode: `Beside` and `Active` match no group,
 * so they open a tab of their own the way a jump to the source should.
 */
async function showWith(
  uri: vscode.Uri,
  column: vscode.ViewColumn,
  editorId: string,
): Promise<void> {
  const current = editorShowing(uri, column);
  if (current !== undefined && current !== editorId) {
    // Reopen With acts on the active editor, so the tab has to come forward as
    // it stands before VSCode is asked to swap what is inside it.
    await openWith(uri, column, current);
    await vscode.commands.executeCommand(REOPEN_ACTIVE_EDITOR_WITH, editorId);
    return;
  }
  await openWith(uri, column, editorId);
}

/**
 * Open the *source* of a typst file, revealing `line` if one is given.
 *
 * Named explicitly rather than left to `vscode.open`, so that a user who has
 * pointed `workbench.editorAssociations` at the preview editor still gets the
 * text editor when they ask to edit.
 */
export async function openSource(
  uri: vscode.Uri,
  column: vscode.ViewColumn,
  line?: number,
): Promise<void> {
  await showWith(uri, column, TEXT_EDITOR);
  if (line === undefined) return;
  const editor = vscode.window.visibleTextEditors.find(
    (candidate) => candidate.document.uri.toString() === uri.toString(),
  );
  editor?.revealRange(
    new vscode.Range(line, 0, line, 0),
    vscode.TextEditorRevealType.AtTop,
  );
}

/**
 * Open a typst file *in* the preview editor — the mirror of `openSource`. The
 * tab holding the source becomes the preview rather than gaining a second tab
 * in front of it.
 */
export async function openPreviewEditor(
  uri: vscode.Uri,
  column: vscode.ViewColumn,
): Promise<void> {
  await showWith(uri, column, PREVIEW_EDITOR_VIEW_TYPE);
}

/**
 * The file shown by the active tab, when that tab is a preview editor. Drives
 * the mode state machine: with a custom editor active there is no
 * `activeTextEditor` to read the current document from.
 */
export function activePreviewEditorUri(): vscode.Uri | undefined {
  const input = vscode.window.tabGroups.activeTabGroup.activeTab?.input;
  if (
    input instanceof vscode.TabInputCustom &&
    input.viewType === PREVIEW_EDITOR_VIEW_TYPE
  ) {
    return input.uri;
  }
  return undefined;
}

/**
 * Whether this file can be opened as a preview editor.
 *
 * The custom editor is contributed for `*.typ` only: a `.typc` is a script, not
 * a document, and has no pages to show.
 */
export function isPreviewable(uri: vscode.Uri): boolean {
  return uri.path.toLowerCase().endsWith('.typ');
}
