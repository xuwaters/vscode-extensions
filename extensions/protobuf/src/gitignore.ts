import * as vscode from 'vscode';

const ALWAYS_EXCLUDED = ['**/node_modules/**', '**/.git/**'];

/**
 * Build a glob pattern suitable for `findFiles()`'s `exclude` parameter by
 * reading `.gitignore` files at each workspace-folder root. Nested gitignores
 * are not traversed — scanning for them would defeat the purpose — but the
 * root gitignore covers the common heavy hitters (build output, caches,
 * vendored dirs, etc.).
 */
export async function buildExcludeGlob(): Promise<string> {
  const patterns = new Set<string>(ALWAYS_EXCLUDED);

  const folders = vscode.workspace.workspaceFolders ?? [];
  for (const folder of folders) {
    const gitignoreUri = vscode.Uri.joinPath(folder.uri, '.gitignore');
    let text: string;
    try {
      const bytes = await vscode.workspace.fs.readFile(gitignoreUri);
      text = new TextDecoder('utf-8').decode(bytes);
    } catch {
      continue;
    }
    for (const glob of gitignoreToGlobs(text)) patterns.add(glob);
  }

  const list = [...patterns];
  return list.length === 1 ? list[0] : `{${list.join(',')}}`;
}

/**
 * Convert `.gitignore` entries into glob patterns for VS Code's `findFiles()`.
 * Covers the common subset — rooted (`/dir`), directory-only (`dir/`), and
 * bare names or wildcard patterns. Negations (`!pattern`) are skipped because
 * `findFiles` has no re-inclusion mechanism; comments and blanks are skipped.
 */
export function gitignoreToGlobs(text: string): string[] {
  const out: string[] = [];
  for (const rawLine of text.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line || line.startsWith('#') || line.startsWith('!')) continue;

    let entry = line.startsWith('\\') ? line.slice(1) : line;
    const rooted = entry.startsWith('/');
    if (rooted) entry = entry.slice(1);
    const dirOnly = entry.endsWith('/');
    if (dirOnly) entry = entry.slice(0, -1);
    if (!entry) continue;

    const prefix = rooted ? '' : '**/';
    if (dirOnly) {
      out.push(`${prefix}${entry}/**`);
    } else {
      out.push(`${prefix}${entry}`);
      out.push(`${prefix}${entry}/**`);
    }
  }
  return out;
}
