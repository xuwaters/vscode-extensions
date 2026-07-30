//! The workspace filesystem as a {@link Vfs}. Paths are `Uri.path` strings
//! carried on top of a base Uri, so a remote or virtual workspace keeps its
//! scheme and authority.

import * as vscode from 'vscode';
import { joinPath } from './paths.js';
import type { Entry, Kind, Vfs } from './vfs.js';

export class VscodeFs implements Vfs {
  constructor(private readonly base: vscode.Uri) {}

  uri(path: string): vscode.Uri {
    return this.base.with({ path, query: '', fragment: '' });
  }

  async kind(path: string): Promise<Kind> {
    const stat = await vscode.workspace.fs.stat(this.uri(path));
    return kindOf(stat.type);
  }

  async readDir(dir: string): Promise<Entry[]> {
    const entries = await vscode.workspace.fs.readDirectory(this.uri(dir));
    return entries.map(([name, type]) => ({
      path: joinPath(dir, name),
      name,
      kind: kindOf(type),
    }));
  }

  async removeDir(dir: string): Promise<void> {
    await vscode.workspace.fs.delete(this.uri(dir), { recursive: false, useTrash: false });
  }

  async readText(path: string): Promise<string> {
    const bytes = await vscode.workspace.fs.readFile(this.uri(path));
    return new TextDecoder('utf-8').decode(bytes);
  }
}

/** `FileType` is a bitmask: a symlinked directory is `Directory | SymbolicLink`. */
function kindOf(type: vscode.FileType): Kind {
  if (type & vscode.FileType.SymbolicLink) return 'symlink';
  if (type & vscode.FileType.Directory) return 'dir';
  return 'file';
}
