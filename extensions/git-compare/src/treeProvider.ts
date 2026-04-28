import * as vscode from 'vscode';
import type { Change, GitAPI, Repository } from './gitApi';
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

export type GroupKind = 'behind' | 'ahead' | 'changed';

interface GroupData {
  kind: GroupKind;
  entries: FileEntry[];
  tree: TreeNode[];
}

export type CompareNode =
  | { kind: 'choose' } // shown when no comparison selected — call to action
  | { kind: 'root'; selection: ComparisonSelection } // shown when a ref is selected
  | { kind: 'message'; message: string }
  | { kind: 'group'; selection: ComparisonSelection; group: GroupKind; fileCount: number }
  | { kind: 'tree'; parent: ComparisonSelection; group: GroupKind; node: TreeNode };

const FILE_CONTEXT = 'gitCompare.file';
const ROOT_CONTEXT = 'gitCompare.root';

export class CompareTreeDataProvider implements vscode.TreeDataProvider<CompareNode> {
  private readonly _onDidChangeTreeData = new vscode.EventEmitter<CompareNode | undefined>();
  readonly onDidChangeTreeData = this._onDidChangeTreeData.event;

  private cachedSelection: ComparisonSelection | undefined;
  private cachedGroups: Record<GroupKind, GroupData> = emptyGroups();
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
        const { selection } = node;
        const repoName = pathBasename(selection.repoRoot);
        const item = new vscode.TreeItem(
          `Compare with: ${selection.label}`,
          vscode.TreeItemCollapsibleState.Expanded,
        );
        const total = this.cachedGroups.changed.entries.length;
        item.description = `${total} file${total === 1 ? '' : 's'} changed`;
        item.iconPath = new vscode.ThemeIcon('git-compare');
        item.tooltip = `Comparing working copy in ${repoName} with ${selection.label} (${selection.ref})\nClick the pencil icon to pick a different ref.`;
        item.contextValue = ROOT_CONTEXT;
        return item;
      }
      case 'group': {
        return groupItem(node, this.cachedGroups[node.group]);
      }
      case 'message': {
        const item = new vscode.TreeItem(node.message, vscode.TreeItemCollapsibleState.None);
        item.iconPath = new vscode.ThemeIcon('info');
        return item;
      }
      case 'tree': {
        return treeNodeItem(node.node, node.parent, node.group);
      }
    }
  }

  async getChildren(element?: CompareNode): Promise<CompareNode[]> {
    if (!element) {
      if (this.gitApi.repositories.length === 0) {
        return [];
      }
      const selection = this.state.get();
      if (!selection) return [{ kind: 'choose' }];

      await this.ensureLoaded(selection);

      if (this.cachedError) {
        return [{ kind: 'message', message: this.cachedError }];
      }

      return [{ kind: 'root', selection }];
    }

    if (element.kind === 'root') {
      if (this.loading) return [{ kind: 'message', message: 'Loading…' }];
      const selection = element.selection;
      const groups: GroupKind[] = ['behind', 'ahead', 'changed'];
      return groups.map((g) => ({
        kind: 'group',
        selection,
        group: g,
        fileCount: this.cachedGroups[g].entries.length,
      }));
    }

    if (element.kind === 'group') {
      const data = this.cachedGroups[element.group];
      if (data.entries.length === 0) {
        return [{ kind: 'message', message: emptyMessage(element.group) }];
      }
      return data.tree.map((n) => ({
        kind: 'tree',
        parent: element.selection,
        group: element.group,
        node: n,
      }));
    }

    if (element.kind === 'tree' && element.node.kind === 'folder') {
      return element.node.children.map((n) => ({
        kind: 'tree',
        parent: element.parent,
        group: element.group,
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
        this.cachedGroups = emptyGroups();
        return;
      }

      const compact = vscode.workspace
        .getConfiguration('gitCompare')
        .get<boolean>('compactFolders', true);

      const [changedChanges, behindAhead] = await Promise.all([
        repo.diffWith(selection.ref),
        loadBehindAhead(repo, selection.ref),
      ]);

      this.cachedGroups = {
        behind: makeGroup('behind', behindAhead.behind, repo.rootUri, compact),
        ahead: makeGroup('ahead', behindAhead.ahead, repo.rootUri, compact),
        changed: makeGroup('changed', changedChanges, repo.rootUri, compact),
      };
      this.cachedSelection = selection;
    } catch (err) {
      this.cachedError = `Failed to compare with ${selection.ref}: ${(err as Error).message}`;
      this.cachedGroups = emptyGroups();
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

function emptyGroups(): Record<GroupKind, GroupData> {
  return {
    behind: { kind: 'behind', entries: [], tree: [] },
    ahead: { kind: 'ahead', entries: [], tree: [] },
    changed: { kind: 'changed', entries: [], tree: [] },
  };
}

function makeGroup(
  kind: GroupKind,
  changes: readonly Change[],
  repoRoot: vscode.Uri,
  compact: boolean,
): GroupData {
  const entries = buildFileEntries(changes, repoRoot);
  const tree = buildTree(entries, compact);
  return { kind, entries, tree };
}

// "Behind" and "Ahead" use triple-dot semantics relative to the merge-base —
// so each side reflects the commits unique to that branch, not the full diff.
// Falls back to empty lists if the histories are unrelated or the API is
// unavailable.
async function loadBehindAhead(
  repo: Repository,
  ref: string,
): Promise<{ behind: Change[]; ahead: Change[] }> {
  try {
    const mergeBase = await repo.getMergeBase('HEAD', ref);
    if (!mergeBase) return { behind: [], ahead: [] };
    const [behind, ahead] = await Promise.all([
      repo.diffBetween(mergeBase, ref),
      repo.diffBetween(mergeBase, 'HEAD'),
    ]);
    return { behind, ahead };
  } catch {
    return { behind: [], ahead: [] };
  }
}

function groupItem(
  node: Extract<CompareNode, { kind: 'group' }>,
  data: GroupData,
): vscode.TreeItem {
  const meta = groupMeta(node.group);
  const collapsed =
    data.entries.length === 0
      ? vscode.TreeItemCollapsibleState.Collapsed
      : meta.expanded
        ? vscode.TreeItemCollapsibleState.Expanded
        : vscode.TreeItemCollapsibleState.Collapsed;
  const item = new vscode.TreeItem(meta.label, collapsed);
  item.iconPath = new vscode.ThemeIcon(meta.icon);
  const count = data.entries.length;
  item.description = `${count} file${count === 1 ? '' : 's'}`;
  item.tooltip = meta.tooltip;
  item.contextValue = `gitCompare.group.${node.group}`;
  return item;
}

function groupMeta(group: GroupKind): {
  label: string;
  icon: string;
  tooltip: string;
  expanded: boolean;
} {
  switch (group) {
    case 'behind':
      return {
        label: 'Behind',
        icon: 'arrow-down',
        tooltip: 'Files changed in commits the compared ref has but HEAD does not',
        expanded: false,
      };
    case 'ahead':
      return {
        label: 'Ahead',
        icon: 'arrow-up',
        tooltip: 'Files changed in commits HEAD has but the compared ref does not',
        expanded: true,
      };
    case 'changed':
      return {
        label: 'Changed Files',
        icon: 'diff',
        tooltip: 'All files differing between the working copy and the compared ref',
        expanded: true,
      };
  }
}

function emptyMessage(group: GroupKind): string {
  switch (group) {
    case 'behind':
      return 'No incoming changes';
    case 'ahead':
      return 'No outgoing changes';
    case 'changed':
      return 'No differences';
  }
}

function treeNodeItem(
  node: TreeNode,
  parent: ComparisonSelection,
  group: GroupKind,
): vscode.TreeItem {
  if (node.kind === 'folder') {
    return folderItem(node);
  }
  return fileItem(node, parent, group);
}

function folderItem(node: FolderTreeNode): vscode.TreeItem {
  const item = new vscode.TreeItem(node.segment, vscode.TreeItemCollapsibleState.Expanded);
  item.iconPath = vscode.ThemeIcon.Folder;
  item.resourceUri = vscode.Uri.from({ scheme: 'gitCompare', path: `/folder/${node.relPath}` });
  item.contextValue = 'gitCompare.folder';
  return item;
}

function fileItem(
  node: FileTreeNode,
  parent: ComparisonSelection,
  group: GroupKind,
): vscode.TreeItem {
  const entry = node.entry;
  const item = new vscode.TreeItem(node.segment, vscode.TreeItemCollapsibleState.None);
  item.resourceUri = entry.change.uri;
  item.description = describeStatus(entry);
  item.tooltip = buildTooltip(entry, parent, group);
  item.contextValue = FILE_CONTEXT;
  item.iconPath = vscode.ThemeIcon.File;
  item.command = {
    command: 'gitCompare.openDiff',
    title: 'Open Diff',
    arguments: [{ entry, parent, group } satisfies FileNodePayload],
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

function buildTooltip(entry: FileEntry, parent: ComparisonSelection, group: GroupKind): string {
  const lines = [
    `${entry.relPath}`,
    `${entry.originalRelPath ? `Renamed from ${entry.originalRelPath}` : statusName(entry)}`,
    groupTooltipLine(group, parent),
  ];
  return lines.join('\n');
}

function groupTooltipLine(group: GroupKind, parent: ComparisonSelection): string {
  switch (group) {
    case 'behind':
      return `Behind ${parent.label} (incoming)`;
    case 'ahead':
      return `Ahead of ${parent.label} (outgoing)`;
    case 'changed':
      return `Comparing with ${parent.label}`;
  }
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
  group: GroupKind;
}
