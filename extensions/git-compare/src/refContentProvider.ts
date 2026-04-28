import * as vscode from 'vscode';
import type { GitAPI, Repository } from './gitApi';

export const REF_SCHEME = 'git-compare-ref';

interface RefUriQuery {
  repoRoot: string;
  ref: string;
  path: string;
}

class RefContentProvider implements vscode.TextDocumentContentProvider {
  constructor(private readonly api: GitAPI) {}

  async provideTextDocumentContent(uri: vscode.Uri): Promise<string> {
    const query = parseQuery(uri);
    if (!query) return '';
    const repo = this.findRepo(query.repoRoot);
    if (!repo) return '';
    try {
      return await repo.show(query.ref, query.path);
    } catch {
      return '';
    }
  }

  private findRepo(repoRoot: string): Repository | undefined {
    return this.api.repositories.find((r) => r.rootUri.fsPath === repoRoot);
  }
}

export function registerRefContentProvider(
  context: vscode.ExtensionContext,
  api: GitAPI,
): void {
  context.subscriptions.push(
    vscode.workspace.registerTextDocumentContentProvider(REF_SCHEME, new RefContentProvider(api)),
  );
}

// Build a URI whose basename is `<filename> (<label>)` so the editor tab
// reads e.g. `extension.ts (origin~main)` — the original filename stays
// intact (the user reads it the same way they'd read it in the explorer),
// and the ref tag sits at the end as a visible annotation. The trade-off is
// that VSCode can't infer the language from the extension anymore, so the
// caller is expected to pair this with an explicit `setTextDocumentLanguage`.
export function buildRefUri(opts: {
  repoRoot: string;
  ref: string;
  filePath: string; // absolute fs path of the working-tree file
  relPath: string; // path inside the repo, '/'-separated
  label: string; // human-friendly ref label (branch / tag / sha)
}): vscode.Uri {
  const filename = baseName(opts.relPath);
  const safeLabel = sanitizeLabel(opts.label);
  const tabName = `${filename} (${safeLabel})`;
  // Keep the original repo-relative directory in the path so multiple files
  // with the same name from different folders don't collide; place tabName at
  // the end so it becomes the tab title.
  const dir = filename === opts.relPath ? '' : opts.relPath.slice(0, -filename.length);
  const path = `/${dir}${tabName}`;
  const query: RefUriQuery = {
    repoRoot: opts.repoRoot,
    ref: opts.ref,
    path: opts.filePath,
  };
  return vscode.Uri.from({
    scheme: REF_SCHEME,
    path,
    query: JSON.stringify(query),
  });
}

function parseQuery(uri: vscode.Uri): RefUriQuery | undefined {
  try {
    return JSON.parse(uri.query) as RefUriQuery;
  } catch {
    return undefined;
  }
}

function sanitizeLabel(label: string): string {
  // `/` in ref names like `origin/main` would break the URI path semantics
  // and look weird in a tab. `~` is conventional shorthand in git output.
  return label.replace(/\//g, '~');
}

function baseName(relPath: string): string {
  const idx = relPath.lastIndexOf('/');
  return idx === -1 ? relPath : relPath.slice(idx + 1);
}
