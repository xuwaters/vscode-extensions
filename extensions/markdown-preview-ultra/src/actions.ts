import * as vscode from 'vscode';
import type { ToggleTaskMessage } from './messages';

/** Verifies a line is a task-list item before the one-character toggle edit. */
const TASK_LINE = /^(\s*(?:[-*+]|\d+[.)])\s+)\[[ xX]\]/;

/** Resolve a workspace-relative or document-relative link destination. */
export function resolveLink(
  document: vscode.TextDocument,
  href: string,
): vscode.Uri {
  // `other.md#section` names a file plus an anchor; only the file resolves.
  // Link destinations are percent-encoded (`my%20notes.md`) but URI paths
  // are held decoded, so undo that before joining.
  const raw = href.replace(/[#?].*$/, '');
  let file: string;
  try {
    file = decodeURIComponent(raw);
  } catch {
    file = raw;
  }
  const base = vscode.Uri.joinPath(document.uri, '..');
  return file.startsWith('/')
    ? vscode.Uri.joinPath(
        vscode.workspace.getWorkspaceFolder(document.uri)?.uri ?? base,
        file.replace(/^\/+/, ''),
      )
    : vscode.Uri.joinPath(base, file);
}

/**
 * The one write path: flip `[ ]`/`[x]` after re-verifying the line. Opt-in via
 * `taskLists.toggleFromPreview`; a stale line number from a page that has not
 * caught up with an edit is dropped rather than applied to whatever moved into
 * its place.
 */
export async function applyTaskToggle(
  document: vscode.TextDocument,
  msg: ToggleTaskMessage,
): Promise<void> {
  const cfg = vscode.workspace.getConfiguration('markdownPreviewUltra');
  if (!cfg.get<boolean>('taskLists.toggleFromPreview', false)) return;
  if (msg.line >= document.lineCount) return;
  const line = document.lineAt(msg.line);
  const match = TASK_LINE.exec(line.text);
  if (!match) return;
  const checkboxChar = match[1].length + 1;
  const edit = new vscode.WorkspaceEdit();
  edit.replace(
    document.uri,
    new vscode.Range(msg.line, checkboxChar, msg.line, checkboxChar + 1),
    msg.checked ? 'x' : ' ',
  );
  await vscode.workspace.applyEdit(edit);
}
