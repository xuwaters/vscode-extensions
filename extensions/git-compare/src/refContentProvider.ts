import * as vscode from 'vscode';
import type { GitAPI, Repository } from './gitApi';

export const REF_SCHEME = 'git-compare-ref';

export type RefSide = 'working' | 'compare';

interface RefUriQuery {
  repoRoot: string;
  ref: string;
  path: string;
  label: string;
  side: RefSide;
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

// Build a URI whose basename is the original filename so VSCode can infer
// the language from the extension. The ref label is carried in the query
// for the FileDecorationProvider to surface as a badge + tooltip on the tab.
// Same path opened at different refs yields distinct URIs (different query),
// so they appear as separate documents.
export function buildRefUri(opts: {
  repoRoot: string;
  ref: string;
  filePath: string; // absolute fs path of the working-tree file
  relPath: string; // path inside the repo, '/'-separated
  label: string; // human-friendly ref label (branch / tag / sha)
  side: RefSide; // which side of the comparison this URI represents
}): vscode.Uri {
  const query: RefUriQuery = {
    repoRoot: opts.repoRoot,
    ref: opts.ref,
    path: opts.filePath,
    label: opts.label,
    side: opts.side,
  };
  return vscode.Uri.from({
    scheme: REF_SCHEME,
    path: `/${opts.relPath}`,
    query: JSON.stringify(query),
  });
}

export interface RefUriInfo {
  label: string;
  side: RefSide;
}

export function parseRefUriInfo(uri: vscode.Uri): RefUriInfo | undefined {
  const q = parseQuery(uri);
  if (!q) return undefined;
  return { label: q.label, side: q.side };
}

function parseQuery(uri: vscode.Uri): RefUriQuery | undefined {
  try {
    return JSON.parse(uri.query) as RefUriQuery;
  } catch {
    return undefined;
  }
}
