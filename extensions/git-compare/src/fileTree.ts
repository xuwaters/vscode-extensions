import * as vscode from 'vscode';
import type { Change } from './gitApi';
import { isAddition, isDeletion, statusLetter } from './gitApi';

export interface FileEntry {
  change: Change;
  // Path relative to the repo root, using '/' separators.
  relPath: string;
  // For renames: the original path (also relative to repo root).
  originalRelPath?: string;
}

export interface FolderTreeNode {
  kind: 'folder';
  // Display segment(s) shown on the folder row. May contain '/' when single
  // children are collapsed.
  segment: string;
  // Full relative path from repo root, used as a stable key.
  relPath: string;
  children: TreeNode[];
}

export interface FileTreeNode {
  kind: 'file';
  segment: string;
  entry: FileEntry;
}

export type TreeNode = FolderTreeNode | FileTreeNode;

export function buildFileEntries(
  changes: readonly Change[],
  repoRoot: vscode.Uri,
): FileEntry[] {
  const rootPath = repoRoot.fsPath;
  const entries: FileEntry[] = [];
  for (const change of changes) {
    const target = change.renameUri ?? change.uri;
    const relPath = relativize(rootPath, target.fsPath);
    if (relPath === undefined) continue;
    const originalRelPath = change.renameUri
      ? relativize(rootPath, change.originalUri.fsPath)
      : undefined;
    entries.push({ change, relPath, originalRelPath });
  }
  entries.sort((a, b) => a.relPath.localeCompare(b.relPath));
  return entries;
}

export function buildTree(entries: readonly FileEntry[], compactFolders: boolean): TreeNode[] {
  const root: FolderTreeNode = { kind: 'folder', segment: '', relPath: '', children: [] };

  for (const entry of entries) {
    insertEntry(root, entry);
  }

  sortChildren(root);
  if (compactFolders) {
    compact(root);
  }
  return root.children;
}

function insertEntry(root: FolderTreeNode, entry: FileEntry): void {
  const parts = entry.relPath.split('/');
  let cursor = root;
  for (let i = 0; i < parts.length - 1; i++) {
    const segment = parts[i];
    const childPath = cursor.relPath ? `${cursor.relPath}/${segment}` : segment;
    let next = cursor.children.find(
      (n): n is FolderTreeNode => n.kind === 'folder' && n.segment === segment,
    );
    if (!next) {
      next = { kind: 'folder', segment, relPath: childPath, children: [] };
      cursor.children.push(next);
    }
    cursor = next;
  }
  cursor.children.push({ kind: 'file', segment: parts[parts.length - 1], entry });
}

function sortChildren(node: FolderTreeNode): void {
  node.children.sort((a, b) => {
    if (a.kind !== b.kind) return a.kind === 'folder' ? -1 : 1;
    return a.segment.localeCompare(b.segment);
  });
  for (const child of node.children) {
    if (child.kind === 'folder') sortChildren(child);
  }
}

function compact(node: FolderTreeNode): void {
  for (const child of node.children) {
    if (child.kind === 'folder') compact(child);
  }
  // Compact a folder whose only child is another folder by merging segments.
  for (let i = 0; i < node.children.length; i++) {
    let child = node.children[i];
    while (
      child.kind === 'folder' &&
      child.children.length === 1 &&
      child.children[0].kind === 'folder'
    ) {
      const inner = child.children[0];
      child = {
        kind: 'folder',
        segment: `${child.segment}/${inner.segment}`,
        relPath: inner.relPath,
        children: inner.children,
      };
    }
    node.children[i] = child;
  }
}

function relativize(rootFsPath: string, fileFsPath: string): string | undefined {
  const root = normalize(rootFsPath);
  const file = normalize(fileFsPath);
  if (file === root) return '';
  const prefix = root.endsWith('/') ? root : `${root}/`;
  if (!file.startsWith(prefix)) return undefined;
  return file.slice(prefix.length);
}

function normalize(p: string): string {
  return p.replace(/\\/g, '/');
}

export function describeStatus(entry: FileEntry): string {
  if (entry.originalRelPath) {
    return `R · ${entry.originalRelPath} →`;
  }
  return statusLetter(entry.change.status);
}

export function entryKey(entry: FileEntry): string {
  return `${entry.relPath}\0${entry.originalRelPath ?? ''}`;
}

export { isAddition, isDeletion };
