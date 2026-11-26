import * as path from 'node:path';
import * as vscode from 'vscode';
import type { Branch, Commit, Ref, Repository } from './gitApi';
import { RefType } from './gitApi';

export interface RefPickResult {
  ref: string;
  label: string;
}

interface RefQuickPickItem extends vscode.QuickPickItem {
  ref?: string;
  enterCommitish?: boolean;
}

interface CommitQuickPickItem extends vscode.QuickPickItem {
  commit?: Commit;
  typed?: string;
  loadMore?: boolean;
}

const DEFAULT_BRANCH_NAMES = new Set(['main', 'master', 'trunk', 'develop']);

// How many commits the history picker lists at first, and how many more each
// "Load more" adds. `git log` is re-run with a larger limit rather than with
// `--skip`, since older vscode.git builds ignore the skip option.
const HISTORY_PAGE_SIZE = 50;

// Sentinels for hand-offs between the two pickers.
const SHOW_HISTORY = Symbol('showHistory');
const GO_BACK = Symbol('goBack');

export async function pickRef(repo: Repository): Promise<RefPickResult | undefined> {
  // The ref picker can hand off to the commit-history picker, whose Back
  // button returns to the ref picker.
  for (;;) {
    const picked = await pickFromRefs(repo);
    if (picked !== SHOW_HISTORY) return picked;
    const commit = await pickFromHistory(repo);
    if (commit !== GO_BACK) return commit;
  }
}

async function pickFromRefs(
  repo: Repository,
): Promise<RefPickResult | typeof SHOW_HISTORY | undefined> {
  const head = repo.state.HEAD;

  // Show the picker immediately, then stream refs in once they load. This way
  // the user sees the picker right away — instead of waiting for git to list
  // every ref before the popup even appears — and on a slow first call (a big
  // repo, cold ref cache) we don't make them stare at a blocked UI.
  const qp = vscode.window.createQuickPick<RefQuickPickItem>();
  qp.title = `Compare ${repoName(repo)} (${displayHead(head?.name, head?.commit)}) working copy with...`;
  qp.placeholder = 'Loading refs...';
  qp.matchOnDescription = true;
  qp.matchOnDetail = true;
  qp.busy = true;
  let items: RefQuickPickItem[] = [makeEnterItem(qp.value)];
  qp.items = items;

  const refreshEnterItem = (): void => {
    for (let i = 0; i < items.length; i++) {
      if (items[i].enterCommitish) {
        items[i] = makeEnterItem(qp.value);
        qp.items = items;
        return;
      }
    }
  };

  const result = new Promise<RefPickResult | typeof SHOW_HISTORY | undefined>((resolve) => {
    let resolved = false;
    const finish = (value: RefPickResult | typeof SHOW_HISTORY | undefined): void => {
      resolved = true;
      qp.hide();
      resolve(value);
    };
    qp.onDidChangeValue(() => refreshEnterItem());
    qp.onDidAccept(() => {
      const picked = qp.activeItems[0];
      const typed = qp.value.trim();
      if (picked?.enterCommitish) {
        if (typed) finish({ ref: typed, label: typed });
        else finish(SHOW_HISTORY);
        return;
      }
      if (picked?.ref) {
        finish({ ref: picked.ref, label: stripIcon(picked.label) });
        return;
      }
      // No item highlighted but user typed something — treat as commit-ish.
      if (typed) finish({ ref: typed, label: typed });
    });
    qp.onDidHide(() => {
      if (!resolved) resolve(undefined);
      qp.dispose();
    });
  });

  qp.show();

  try {
    const refs = await collectRefs(repo);
    items = buildItems(refs, head, qp.value);
    qp.items = items;
    qp.placeholder = 'Select a branch, tag, or commit — or type a SHA / branch name';
  } catch (err) {
    qp.placeholder = `Failed to list refs: ${(err as Error).message}`;
  } finally {
    qp.busy = false;
  }

  return result;
}

function makeEnterItem(typed: string): RefQuickPickItem {
  const v = typed.trim();
  if (v) {
    return {
      label: `$(arrow-right) Use "${v}"`,
      description: 'Compare against this branch, tag, or commit SHA',
      enterCommitish: true,
      alwaysShow: true,
    };
  }
  return {
    label: '$(history) Select from commit history...',
    description: 'or type a branch, tag, or commit SHA',
    enterCommitish: true,
    alwaysShow: true,
  };
}

async function collectRefs(repo: Repository): Promise<Ref[]> {
  // Prefer the on-demand `getRefs` API — `state.refs` is only the cached
  // working copy of what the git extension has already fetched, which on a
  // cold load can be a strict subset (often "current branch only").
  if (repo.getRefs) {
    try {
      const refs = await repo.getRefs({ sort: 'alphabetically' });
      if (refs.length > 0) return refs;
    } catch {
      // Fall through to the cached list and to getBranches() below.
    }
  }

  const merged = new Map<string, Ref>();
  for (const r of repo.state.refs) {
    const key = refKey(r);
    if (key) merged.set(key, r);
  }
  if (repo.getBranches) {
    try {
      const local = await repo.getBranches({ remote: false, sort: 'alphabetically' });
      for (const r of local) {
        const key = refKey(r);
        if (key) merged.set(key, r);
      }
      const remote = await repo.getBranches({ remote: true, sort: 'alphabetically' });
      for (const r of remote) {
        const key = refKey(r);
        if (key) merged.set(key, r);
      }
    } catch {
      // ignore — we already have whatever state.refs gave us
    }
  }
  return Array.from(merged.values());
}

function refKey(r: Ref): string | undefined {
  if (!r.name) return undefined;
  return `${r.type}:${r.name}`;
}

interface RefGroup {
  label: string;
  items: RefQuickPickItem[];
}

type NamedRef = Ref & { name: string };

// Lists the refs in labelled groups, the most likely comparison first, and
// ends with the "type a ref / browse history" entry.
function buildItems(
  refs: readonly Ref[],
  head: Branch | undefined,
  typed: string,
): RefQuickPickItem[] {
  const upstreams = upstreamItems(refs, head);
  const otherLocals = refsOfType(refs, RefType.Head).filter((r) => r.name !== head?.name);
  const isDefault = (r: NamedRef): boolean => DEFAULT_BRANCH_NAMES.has(r.name);

  const groups: RefGroup[] = [
    { label: 'Upstream', items: upstreams },
    { label: 'Current branch', items: currentBranchItems(refs, head) },
    { label: 'Default branches', items: branchItems(otherLocals.filter(isDefault)) },
    { label: 'Local branches', items: branchItems(otherLocals.filter((r) => !isDefault(r))) },
    { label: 'Remote branches', items: remoteBranchItems(refs, new Set(upstreams.map((i) => i.ref))) },
    { label: 'Tags', items: refsOfType(refs, RefType.Tag).map((r) => refItem(r, '$(tag)')) },
  ];

  return [
    ...flattenGroups(groups),
    { label: '', kind: vscode.QuickPickItemKind.Separator },
    makeEnterItem(typed),
  ];
}

function refsOfType(refs: readonly Ref[], type: RefType): NamedRef[] {
  return refs.filter((r): r is NamedRef => r.type === type && !!r.name);
}

// The upstream of the *current* branch is the most common comparison ("what
// would I push?"). Use the branch's configured upstream; only guess by name
// when none is set, since several remotes can carry a branch of the same name.
function upstreamItems(refs: readonly Ref[], head: Branch | undefined): RefQuickPickItem[] {
  const remotes = refsOfType(refs, RefType.RemoteHead);
  if (head?.upstream) {
    const upstreamName = `${head.upstream.remote}/${head.upstream.name}`;
    return remotes
      .filter((r) => r.name === upstreamName)
      .map((r) => refItem(r, '$(cloud-download)', 'Upstream of current branch'));
  }
  const headName = head?.name;
  if (!headName) return [];
  return remotes
    .filter((r) => r.name.endsWith(`/${headName}`))
    .map((r) => refItem(r, '$(cloud-download)', 'Same name as current branch'));
}

// Comparing the working copy with the current branch shows the uncommitted
// changes.
function currentBranchItems(refs: readonly Ref[], head: Branch | undefined): RefQuickPickItem[] {
  if (!head?.name) return [];
  const current = refs.find((r) => r.type === RefType.Head && r.name === head.name) ?? head;
  return [refItem(current, '$(git-branch)', 'uncommitted changes', current.commit ?? head.commit)];
}

function branchItems(branches: readonly NamedRef[]): RefQuickPickItem[] {
  return branches.map((r) => refItem(r, '$(git-branch)'));
}

// Skips refs already listed under Upstream, and `<remote>/HEAD`, which is only
// an alias for that remote's default branch.
function remoteBranchItems(
  refs: readonly Ref[],
  upstreams: ReadonlySet<string | undefined>,
): RefQuickPickItem[] {
  return refsOfType(refs, RefType.RemoteHead)
    .filter((r) => !upstreams.has(r.name) && !r.name.endsWith('/HEAD'))
    .map((r) => refItem(r, '$(cloud)'));
}

// Drops empty groups and heads each remaining one with a labelled separator.
function flattenGroups(groups: readonly RefGroup[]): RefQuickPickItem[] {
  return groups
    .filter((g) => g.items.length > 0)
    .flatMap((g): RefQuickPickItem[] => [
      { label: g.label, kind: vscode.QuickPickItemKind.Separator },
      ...g.items,
    ]);
}

// What the history picker has loaded so far.
interface HistoryState {
  commits: Commit[];
  // How many commits the next `git log` asks for.
  limit: number;
  // Whether the last load filled `limit`, so older commits may exist.
  hasMore: boolean;
  loading: boolean;
}

type HistoryPickResult = RefPickResult | typeof GO_BACK | undefined;

async function pickFromHistory(repo: Repository): Promise<HistoryPickResult> {
  const qp = createHistoryQuickPick(repo);
  const history: HistoryState = { commits: [], limit: HISTORY_PAGE_SIZE, hasMore: false, loading: false };

  const loadMore = (): void => {
    if (history.loading) return;
    history.limit += HISTORY_PAGE_SIZE;
    void loadHistory(repo, qp, history);
  };

  const result = new Promise<HistoryPickResult>((resolve) => {
    let resolved = false;
    const finish = (value: HistoryPickResult): void => {
      resolved = true;
      qp.hide();
      resolve(value);
    };
    qp.onDidChangeValue(() => renderHistory(qp, history));
    qp.onDidTriggerButton((button) => {
      if (button === vscode.QuickInputButtons.Back) finish(GO_BACK);
    });
    qp.onDidAccept(() => {
      const picked = qp.activeItems[0];
      if (picked?.loadMore) {
        loadMore();
        return;
      }
      const chosen = historyPickResult(picked, qp.value);
      if (chosen) finish(chosen);
    });
    qp.onDidHide(() => {
      if (!resolved) resolve(undefined);
      qp.dispose();
    });
  });

  qp.show();
  await loadHistory(repo, qp, history);
  return result;
}

function createHistoryQuickPick(repo: Repository): vscode.QuickPick<CommitQuickPickItem> {
  const qp = vscode.window.createQuickPick<CommitQuickPickItem>();
  const head = repo.state.HEAD;
  qp.title = `Compare ${repoName(repo)} with a commit on ${displayHead(head?.name, head?.commit)}`;
  qp.placeholder = 'Loading commits...';
  qp.matchOnDescription = true;
  qp.keepScrollPosition = true;
  qp.buttons = [vscode.QuickInputButtons.Back];
  qp.busy = true;
  return qp;
}

// Re-reads the history up to `history.limit` commits and re-renders the picker.
async function loadHistory(
  repo: Repository,
  qp: vscode.QuickPick<CommitQuickPickItem>,
  history: HistoryState,
): Promise<void> {
  const previous = history.commits.length;
  history.loading = true;
  qp.busy = true;
  renderHistory(qp, history);
  try {
    history.commits = await repo.log({ maxEntries: history.limit });
    history.hasMore = history.commits.length >= history.limit;
    qp.placeholder = history.commits.length
      ? 'Select a commit — or type a SHA / branch name'
      : 'No commits yet — type a branch, tag, or commit SHA';
  } catch (err) {
    history.hasMore = false;
    qp.placeholder = `Failed to read commit history: ${(err as Error).message}`;
  } finally {
    history.loading = false;
    qp.busy = false;
  }
  // After "Load more", land on the first newly loaded commit so the user
  // can keep scrolling from where the old list ended.
  renderHistory(qp, history, previous > 0 ? history.commits[previous] : undefined);
}

function renderHistory(
  qp: vscode.QuickPick<CommitQuickPickItem>,
  history: HistoryState,
  focus?: Commit,
): void {
  const items = historyItems(history, qp.value.trim());
  qp.items = items;
  const target = focus && items.find((i) => i.commit === focus);
  if (target) qp.activeItems = [target];
}

// The typed ref (if any), the loaded commits, then "Load more" when older
// commits may exist.
function historyItems(history: HistoryState, typed: string): CommitQuickPickItem[] {
  const items: CommitQuickPickItem[] = [];
  if (typed) {
    items.push({
      label: `$(arrow-right) Use "${typed}"`,
      description: 'Compare against this branch, tag, or commit SHA',
      typed,
      alwaysShow: true,
    });
  }
  items.push(...history.commits.map(commitItem));
  if (history.hasMore) {
    items.push(
      { label: '', kind: vscode.QuickPickItemKind.Separator },
      {
        label: history.loading ? '$(loading~spin) Loading...' : '$(fold-down) Load more commits...',
        description: `showing the latest ${history.commits.length}`,
        loadMore: true,
        alwaysShow: true,
      },
    );
  }
  return items;
}

// The highlighted commit, else the typed ref.
function historyPickResult(
  picked: CommitQuickPickItem | undefined,
  value: string,
): RefPickResult | undefined {
  if (picked?.commit) return { ref: picked.commit.hash, label: picked.commit.hash.slice(0, 8) };
  const typed = picked?.typed ?? value.trim();
  return typed ? { ref: typed, label: typed } : undefined;
}

function commitItem(commit: Commit): CommitQuickPickItem {
  const subject = commit.message.split('\n', 1)[0].trim() || '(no message)';
  const meta = [commit.hash.slice(0, 8)];
  if (commit.authorName) meta.push(commit.authorName);
  const date = commit.authorDate ?? commit.commitDate;
  if (date) meta.push(relativeTime(date));
  return {
    label: `$(git-commit) ${subject}`,
    description: meta.join(' · '),
    commit,
  };
}

const RELATIVE_UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
  ['year', 365 * 24 * 3600],
  ['month', 30 * 24 * 3600],
  ['week', 7 * 24 * 3600],
  ['day', 24 * 3600],
  ['hour', 3600],
  ['minute', 60],
];

const relativeFormat = new Intl.RelativeTimeFormat(undefined, { numeric: 'auto' });

function relativeTime(date: Date): string {
  const seconds = (date.getTime() - Date.now()) / 1000;
  for (const [unit, size] of RELATIVE_UNITS) {
    if (Math.abs(seconds) >= size) return relativeFormat.format(Math.round(seconds / size), unit);
  }
  return relativeFormat.format(0, 'second');
}

// The description is the short SHA, followed by `note` when given.
function refItem(ref: Ref, icon: string, note?: string, commit = ref.commit): RefQuickPickItem {
  const name = ref.name ?? commit?.slice(0, 8) ?? '';
  const parts = [commit?.slice(0, 8), note].filter(Boolean);
  return {
    label: `${icon} ${name}`,
    description: parts.length ? parts.join(' · ') : undefined,
    ref: name,
  };
}

function stripIcon(label: string): string {
  return label.replace(/^\$\([^)]+\)\s*/, '');
}

function repoName(repo: Repository): string {
  return path.basename(repo.rootUri.fsPath);
}

function displayHead(name: string | undefined, commit: string | undefined): string {
  return name ?? commit?.slice(0, 8) ?? 'HEAD';
}

export async function pickRepository(
  repos: readonly Repository[],
): Promise<Repository | undefined> {
  if (repos.length === 0) return undefined;
  if (repos.length === 1) return repos[0];

  const items = repos.map((r) => ({
    label: `$(repo) ${vscode.workspace.asRelativePath(r.rootUri, true)}`,
    description: r.state.HEAD?.name ?? r.state.HEAD?.commit?.slice(0, 8),
    repo: r,
  }));
  const picked = await vscode.window.showQuickPick(items, {
    title: 'Select repository',
    placeHolder: 'Choose a repository to compare against',
  });
  return picked?.repo;
}
