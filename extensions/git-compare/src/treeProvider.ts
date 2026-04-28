import * as vscode from 'vscode';
import type { GitAPI, Repository } from './gitApi';
import {
  type FileEntry,
  type FileTreeNode,
  type FolderTreeNode,
  type TreeNode,
  buildFileEntries,
  buildTree,
  describeStatus,
  isAddition,
  isDeletion,
} from './fileTree';
import { CompareState, type ComparisonSelection } from './state';

export type CompareNode =
  | { kind: 'choose' } // shown when no comparison selected — call to action
  | { kind: 'root'; selection: ComparisonSelection; fileCount: number } // shown when a ref is selected
  | { kind: 'message'; message: string }
  | { kind: 'tree'; parent: ComparisonSelection; node: TreeNode };

const FILE_CONTEXT = 'gitCompare.file';
const ROOT_CONTEXT = 'gitCompare.root';

export class CompareTreeDataProvider implements vscode.TreeDataProvider<CompareNode> {
  private readonly _onDidChangeTreeData = new vscode.EventEmitter<CompareNode | undefined>();
  readonly onDidChangeTreeData = this._onDidChangeTreeData.event;

  private cachedSelection: ComparisonSelection | undefined;
  private cachedEntries: FileEntry[] = [];
  private cachedTree: TreeNode[] = [];
  private cachedError: string | undefined;
  private loading = false;

  constructor(
    private readonly gitApi: GitAPI,
    private readonly state: CompareState,
  ) {}

  refresh(): void {
    this.cachedSelection = undefined;
    this._onDidChangeTreeData.fire(undefined);
  }

  getTreeItem(node: CompareNode): vscode.TreeItem {
    switch (node.kind) {
      case 'choose': {
        const item = new vscode.TreeItem('Compare with…', vscode.TreeItemCollapsibleState.None);
        item.iconPath = new vscode.ThemeIcon('git-compare');
        item.tooltip = 'Pick a branch, tag, or commit to compare the working copy against';
        item.command = {
          command: 'gitCompare.choose',
          title: 'Compare with…',
        };
        item.contextValue = 'gitCompare.choose';
        return item;
      }
      case 'root': {
        const { selection, fileCount } = node;
        const repoName = pathBasename(selection.repoRoot);
        const item = new vscode.TreeItem(
          `Compare with: ${selection.label}`,
          vscode.TreeItemCollapsibleState.Expanded,
        );
        item.description = `${fileCount} file${fileCount === 1 ? '' : 's'} changed`;
        item.iconPath = new vscode.ThemeIcon('git-compare');
        item.tooltip = `Comparing working copy in ${repoName} with ${selection.label} (${selection.ref})\nClick the pencil icon to pick a different ref.`;
        // Expose a stable contextValue so the inline "change ref" icon hooks
        // in via package.json's view/item/context contribution.
        item.contextValue = ROOT_CONTEXT;
        return item;
      }
      case 'message': {
        const item = new vscode.TreeItem(node.message, vscode.TreeItemCollapsibleState.None);
        item.iconPath = new vscode.ThemeIcon('info');
        return item;
      }
      case 'tree': {
        return treeNodeItem(node.node, node.parent);
      }
    }
  }

  async getChildren(element?: CompareNode): Promise<CompareNode[]> {
    if (!element) {
      if (this.gitApi.repositories.length === 0) {
        // Returning empty lets the registered viewsWelcome message show.
        return [];
      }
      const selection = this.state.get();
      if (!selection) return [{ kind: 'choose' }];

      await this.ensureLoaded(selection);

      if (this.cachedError) {
        return [{ kind: 'message', message: this.cachedError }];
      }

      // Single root row that announces the current comparison and acts as
      // the parent of the file tree — collapsing the previous two-row layout
      // (chooser + header) into one self-describing item.
      return [
        {
          kind: 'root',
          selection,
          fileCount: this.cachedEntries.length,
        },
      ];
    }

    if (element.kind === 'root') {
      if (this.loading) return [{ kind: 'message', message: 'Loading…' }];
      if (this.cachedEntries.length === 0) {
        return [{ kind: 'message', message: 'No differences' }];
      }
      return this.cachedTree.map((n) => ({ kind: 'tree', parent: element.selection, node: n }));
    }

    if (element.kind === 'tree' && element.node.kind === 'folder') {
      return element.node.children.map((n) => ({
        kind: 'tree',
        parent: element.parent,
        node: n,
      }));
    }

    return [];
  }

  private async ensureLoaded(selection: ComparisonSelection): Promise<void> {
    if (
      this.cachedSelection &&
      this.cachedSelection.repoRoot === selection.repoRoot &&
      this.cachedSelection.ref === selection.ref &&
      !this.loading
    ) {
      return;
    }
    this.loading = true;
    this.cachedError = undefined;
    try {
      const repo = this.findRepo(selection.repoRoot);
      if (!repo) {
        this.cachedError = `Repository not found: ${selection.repoRoot}`;
        this.cachedEntries = [];
        this.cachedTree = [];
        return;
      }
      const changes = await repo.diffWith(selection.ref);
      this.cachedEntries = buildFileEntries(changes, repo.rootUri);
      const compact = vscode.workspace
        .getConfiguration('gitCompare')
        .get<boolean>('compactFolders', true);
      this.cachedTree = buildTree(this.cachedEntries, compact);
      this.cachedSelection = selection;
    } catch (err) {
      this.cachedError = `Failed to compare with ${selection.ref}: ${(err as Error).message}`;
      this.cachedEntries = [];
      this.cachedTree = [];
    } finally {
      this.loading = false;
    }
  }

  private findRepo(repoRoot: string): Repository | undefined {
    return this.gitApi.repositories.find((r) => r.rootUri.fsPath === repoRoot);
  }

  /** Re-read the currently selected comparison from disk and refire. */
  invalidate(): void {
    this.cachedSelection = undefined;
    this._onDidChangeTreeData.fire(undefined);
  }
}

function treeNodeItem(node: TreeNode, parent: ComparisonSelection): vscode.TreeItem {
  if (node.kind === 'folder') {
    return folderItem(node);
  }
  return fileItem(node, parent);
}

function folderItem(node: FolderTreeNode): vscode.TreeItem {
  const item = new vscode.TreeItem(node.segment, vscode.TreeItemCollapsibleState.Expanded);
  item.iconPath = vscode.ThemeIcon.Folder;
  item.resourceUri = vscode.Uri.from({ scheme: 'gitCompare', path: `/folder/${node.relPath}` });
  item.contextValue = 'gitCompare.folder';
  return item;
}

function fileItem(node: FileTreeNode, parent: ComparisonSelection): vscode.TreeItem {
  const entry = node.entry;
  const item = new vscode.TreeItem(node.segment, vscode.TreeItemCollapsibleState.None);
  item.resourceUri = entry.change.uri;
  item.description = describeStatus(entry);
  item.tooltip = buildTooltip(entry, parent);
  item.contextValue = FILE_CONTEXT;
  item.iconPath = vscode.ThemeIcon.File;
  item.command = {
    command: 'gitCompare.openDiff',
    title: 'Open Diff',
    arguments: [{ entry, parent } satisfies FileNodePayload],
  };
  // VSCode colors decorations from the resourceUri, which would clash with the
  // git decoration provider; we keep our own status letter in `description`.
  if (isDeletion(entry.change.status)) {
    item.description = `D · deleted`;
  } else if (isAddition(entry.change.status)) {
    item.description = `A · added`;
  }
  return item;
}

function buildTooltip(entry: FileEntry, parent: ComparisonSelection): string {
  const lines = [
    `${entry.relPath}`,
    `${entry.originalRelPath ? `Renamed from ${entry.originalRelPath}` : statusName(entry)}`,
    `Comparing with ${parent.label}`,
  ];
  return lines.join('\n');
}

function statusName(entry: FileEntry): string {
  if (isAddition(entry.change.status)) return 'Added';
  if (isDeletion(entry.change.status)) return 'Deleted';
  return 'Modified';
}

function pathBasename(p: string): string {
  const norm = p.replace(/\\/g, '/');
  const idx = norm.lastIndexOf('/');
  return idx === -1 ? norm : norm.slice(idx + 1);
}

export interface FileNodePayload {
  entry: FileEntry;
  parent: ComparisonSelection;
}
