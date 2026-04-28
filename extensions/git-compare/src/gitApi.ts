import * as vscode from 'vscode';

// Minimal subset of the vscode.git extension API surface that we consume.
// Mirrors https://github.com/microsoft/vscode/blob/main/extensions/git/src/api/git.d.ts.

export enum RefType {
  Head = 0,
  RemoteHead = 1,
  Tag = 2,
}

export enum Status {
  INDEX_MODIFIED = 0,
  INDEX_ADDED = 1,
  INDEX_DELETED = 2,
  INDEX_RENAMED = 3,
  INDEX_COPIED = 4,
  MODIFIED = 5,
  DELETED = 6,
  UNTRACKED = 7,
  IGNORED = 8,
  INTENT_TO_ADD = 9,
  INTENT_TO_RENAME = 10,
  TYPE_CHANGED = 11,
  ADDED_BY_US = 12,
  ADDED_BY_THEM = 13,
  DELETED_BY_US = 14,
  DELETED_BY_THEM = 15,
  BOTH_ADDED = 16,
  BOTH_DELETED = 17,
  BOTH_MODIFIED = 18,
}

export interface Ref {
  readonly type: RefType;
  readonly name?: string;
  readonly commit?: string;
  readonly remote?: string;
}

export interface Branch extends Ref {
  readonly upstream?: { readonly name: string; readonly remote: string };
  readonly ahead?: number;
  readonly behind?: number;
}

export interface Change {
  readonly uri: vscode.Uri;
  readonly originalUri: vscode.Uri;
  readonly renameUri: vscode.Uri | undefined;
  readonly status: Status;
}

export interface Commit {
  readonly hash: string;
  readonly message: string;
  readonly authorName?: string;
  readonly authorDate?: Date;
}

export interface RepositoryState {
  readonly HEAD: Branch | undefined;
  readonly refs: Ref[];
  readonly remotes: { readonly name: string; readonly fetchUrl?: string; readonly pushUrl?: string }[];
  readonly onDidChange: vscode.Event<void>;
}

export interface RefQuery {
  readonly contains?: string;
  readonly count?: number;
  readonly pattern?: string | string[];
  readonly sort?: 'alphabetically' | 'committerdate';
}

export interface BranchQuery {
  readonly remote?: boolean;
  readonly contains?: string;
  readonly count?: number;
  readonly pattern?: string | string[];
  readonly sort?: 'alphabetically' | 'committerdate';
}

export interface Repository {
  readonly rootUri: vscode.Uri;
  readonly state: RepositoryState;
  diffWith(ref: string): Promise<Change[]>;
  diffWith(ref: string, path: string): Promise<string>;
  diffBetween(ref1: string, ref2: string): Promise<Change[]>;
  diffBetween(ref1: string, ref2: string, path: string): Promise<string>;
  getMergeBase(ref1: string, ref2: string): Promise<string | undefined>;
  getCommit(ref: string): Promise<Commit>;
  show(ref: string, path: string): Promise<string>;
  getRefs?(query: RefQuery, cancellationToken?: vscode.CancellationToken): Promise<Ref[]>;
  getBranches?(query: BranchQuery, cancellationToken?: vscode.CancellationToken): Promise<Ref[]>;
}

export interface GitAPI {
  readonly repositories: Repository[];
  readonly onDidOpenRepository: vscode.Event<Repository>;
  readonly onDidCloseRepository: vscode.Event<Repository>;
}

export interface GitExtension {
  readonly enabled: boolean;
  readonly onDidChangeEnablement: vscode.Event<boolean>;
  getAPI(version: 1): GitAPI;
}

export async function getGitApi(): Promise<GitAPI | undefined> {
  const ext = vscode.extensions.getExtension<GitExtension>('vscode.git');
  if (!ext) return undefined;
  if (!ext.isActive) {
    await ext.activate();
  }
  if (!ext.exports.enabled) return undefined;
  return ext.exports.getAPI(1);
}

// The vscode.git content provider serves any file at any ref via a `git:` URI.
// Format matches what the built-in extension itself produces — opening one
// through `vscode.diff` yields the same editor as the SCM "Open Changes" flow.
export function toGitUri(uri: vscode.Uri, ref: string): vscode.Uri {
  return uri.with({
    scheme: 'git',
    path: uri.path,
    query: JSON.stringify({ path: uri.fsPath, ref }),
  });
}

export function statusLetter(status: Status): string {
  switch (status) {
    case Status.INDEX_ADDED:
    case Status.UNTRACKED:
    case Status.INTENT_TO_ADD:
      return 'A';
    case Status.INDEX_DELETED:
    case Status.DELETED:
      return 'D';
    case Status.INDEX_RENAMED:
    case Status.INTENT_TO_RENAME:
      return 'R';
    case Status.INDEX_COPIED:
      return 'C';
    case Status.TYPE_CHANGED:
      return 'T';
    case Status.INDEX_MODIFIED:
    case Status.MODIFIED:
    default:
      return 'M';
  }
}

export function isDeletion(status: Status): boolean {
  return status === Status.DELETED || status === Status.INDEX_DELETED;
}

export function isAddition(status: Status): boolean {
  return (
    status === Status.INDEX_ADDED ||
    status === Status.UNTRACKED ||
    status === Status.INTENT_TO_ADD
  );
}
