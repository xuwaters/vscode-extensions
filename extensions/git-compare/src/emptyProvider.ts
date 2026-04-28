import * as vscode from 'vscode';

export const EMPTY_SCHEME = 'git-compare-empty';

class EmptyContentProvider implements vscode.TextDocumentContentProvider {
  provideTextDocumentContent(): string {
    return '';
  }
}

export function registerEmptyContentProvider(context: vscode.ExtensionContext): void {
  context.subscriptions.push(
    vscode.workspace.registerTextDocumentContentProvider(EMPTY_SCHEME, new EmptyContentProvider()),
  );
}

// Build a stable empty-content URI for the given path so that the diff editor
// shows a sensible label. Used as the left-hand side for added files and the
// right-hand side for deleted files.
export function emptyUri(displayPath: string, label: string): vscode.Uri {
  return vscode.Uri.from({
    scheme: EMPTY_SCHEME,
    path: `/${label}/${displayPath}`,
  });
}
