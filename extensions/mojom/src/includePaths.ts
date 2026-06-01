import * as vscode from 'vscode';
import * as path from 'path';

/**
 * Resolve `mojom.includePaths` into absolute filesystem paths. The returned
 * list is fed into the analyzer via `set_include_paths`, and is also
 * prepended with every workspace root so that `import "foo/bar.mojom"` can
 * find files in the open workspace without explicit configuration.
 */
export function resolveIncludePaths(): string[] {
  const config = vscode.workspace.getConfiguration('mojom');
  const raw = config.get<string[]>('includePaths', []);
  const out: string[] = [];
  const roots = vscode.workspace.workspaceFolders ?? [];
  for (const p of raw) {
    if (path.isAbsolute(p)) {
      out.push(p);
    } else if (roots.length > 0) {
      out.push(path.resolve(roots[0].uri.fsPath, p));
    } else {
      out.push(p);
    }
  }
  for (const r of roots) {
    out.push(r.uri.fsPath);
  }
  return out;
}
