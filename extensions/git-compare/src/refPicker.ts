import * as vscode from 'vscode';
import type { Commit, Ref, Repository } from './gitApi';
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
  const headName = head?.name;

  // Show the picker immediately, then stream refs in once they load. This way
  // the user sees the picker right away — instead of waiting for git to list
  // every ref before the popup even appears — and on a slow first call (a big
  // repo, cold ref cache) we don't make them stare at a blocked UI.
  const qp = vscode.window.createQuickPick<RefQuickPickItem>();
  qp.title = `Compare working copy (${displayHead(head?.name, head?.commit)}) with…`;
  qp.placeholder = 'Loading refs…';
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
    items = buildItems(refs, headName, qp.value);
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
    label: '$(history) Select from commit history…',
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

function buildItems(
  refs: readonly Ref[],
  headName: string | undefined,
  typed: string,
): RefQuickPickItem[] {
  const buckets: { label: string; items: RefQuickPickItem[] }[] = [];

  const upstreamItems: RefQuickPickItem[] = [];
  // Surface the upstream of the *current* branch first if we can find it in
  // the ref list — that's the most common comparison ("what would I push?").
  for (const ref of refs) {
    if (ref.type === RefType.RemoteHead && ref.name && headName && ref.name.endsWith(`/${headName}`)) {
      upstreamItems.push(refItem(ref, '$(cloud-download)', 'Upstream of current branch'));
    }
  }
  if (upstreamItems.length) buckets.push({ label: 'Upstream', items: upstreamItems });

  const defaults: RefQuickPickItem[] = [];
  for (const ref of refs) {
    if (
      ref.type === RefType.Head &&
      ref.name &&
      DEFAULT_BRANCH_NAMES.has(ref.name) &&
      ref.name !== headName
    ) {
      defaults.push(refItem(ref, '$(git-branch)'));
    }
  }
  if (defaults.length) buckets.push({ label: 'Default branches', items: defaults });

  const localBranches = refs
    .filter(
      (r) =>
        r.type === RefType.Head &&
        r.name &&
        r.name !== headName &&
        !DEFAULT_BRANCH_NAMES.has(r.name),
    )
    .map((r) => refItem(r, '$(git-branch)'));
  if (localBranches.length) buckets.push({ label: 'Local branches', items: localBranches });

  const remoteBranches = refs
    .filter((r) => r.type === RefType.RemoteHead && r.name)
    .map((r) => refItem(r, '$(cloud)'));
  if (remoteBranches.length) buckets.push({ label: 'Remote branches', items: remoteBranches });

  const tags = refs
    .filter((r) => r.type === RefType.Tag && r.name)
    .map((r) => refItem(r, '$(tag)'));
  if (tags.length) buckets.push({ label: 'Tags', items: tags });

  const items: RefQuickPickItem[] = [];
  for (const bucket of buckets) {
    items.push({ label: bucket.label, kind: vscode.QuickPickItemKind.Separator });
    items.push(...bucket.items);
  }
  items.push({ label: '', kind: vscode.QuickPickItemKind.Separator });
  items.push(makeEnterItem(typed));
  return items;
}

async function pickFromHistory(
  repo: Repository,
): Promise<RefPickResult | typeof GO_BACK | undefined> {
  const qp = vscode.window.createQuickPick<CommitQuickPickItem>();
  const head = repo.state.HEAD;
  qp.title = `Compare with a commit on ${displayHead(head?.name, head?.commit)}`;
  qp.placeholder = 'Loading commits…';
  qp.matchOnDescription = true;
  qp.keepScrollPosition = true;
  qp.buttons = [vscode.QuickInputButtons.Back];
  qp.busy = true;

  let commits: Commit[] = [];
  let limit = HISTORY_PAGE_SIZE;
  let hasMore = false;
  let loading = false;

  const render = (focus?: Commit): void => {
    const items: CommitQuickPickItem[] = [];
    const typed = qp.value.trim();
    if (typed) {
      items.push({
        label: `$(arrow-right) Use "${typed}"`,
        description: 'Compare against this branch, tag, or commit SHA',
        typed,
        alwaysShow: true,
      });
    }
    for (const c of commits) items.push(commitItem(c));
    if (hasMore) {
      items.push({ label: '', kind: vscode.QuickPickItemKind.Separator });
      items.push({
        label: loading ? '$(loading~spin) Loading…' : '$(fold-down) Load more commits…',
        description: `showing the latest ${commits.length}`,
        loadMore: true,
        alwaysShow: true,
      });
    }
    qp.items = items;
    if (focus) {
      const target = items.find((i) => i.commit === focus);
      if (target) qp.activeItems = [target];
    }
  };

  const load = async (): Promise<void> => {
    loading = true;
    qp.busy = true;
    render();
    const previous = commits.length;
    try {
      const fetched = await repo.log({ maxEntries: limit });
      commits = fetched;
      hasMore = fetched.length >= limit;
      qp.placeholder = commits.length
        ? 'Select a commit — or type a SHA / branch name'
        : 'No commits yet — type a branch, tag, or commit SHA';
    } catch (err) {
      hasMore = false;
      qp.placeholder = `Failed to read commit history: ${(err as Error).message}`;
    } finally {
      loading = false;
      qp.busy = false;
    }
    // After "Load more", land on the first newly loaded commit so the user
    // can keep scrolling from where the old list ended.
    render(previous > 0 ? commits[previous] : undefined);
  };

  const result = new Promise<RefPickResult | typeof GO_BACK | undefined>((resolve) => {
    let resolved = false;
    const finish = (value: RefPickResult | typeof GO_BACK | undefined): void => {
      resolved = true;
      qp.hide();
      resolve(value);
    };
    qp.onDidChangeValue(() => render());
    qp.onDidTriggerButton((button) => {
      if (button === vscode.QuickInputButtons.Back) finish(GO_BACK);
    });
    qp.onDidAccept(() => {
      const picked = qp.activeItems[0];
      if (picked?.loadMore) {
        if (!loading) {
          limit += HISTORY_PAGE_SIZE;
          void load();
        }
        return;
      }
      if (picked?.commit) {
        finish({ ref: picked.commit.hash, label: picked.commit.hash.slice(0, 8) });
        return;
      }
      const typed = picked?.typed ?? qp.value.trim();
      if (typed) finish({ ref: typed, label: typed });
    });
    qp.onDidHide(() => {
      if (!resolved) resolve(undefined);
      qp.dispose();
    });
  });

  qp.show();
  await load();
  return result;
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

function refItem(ref: Ref, icon: string, description?: string): RefQuickPickItem {
  const name = ref.name ?? ref.commit?.slice(0, 8) ?? '';
  return {
    label: `${icon} ${name}`,
    description: description ?? (ref.commit ? ref.commit.slice(0, 8) : undefined),
    ref: name,
  };
}

function stripIcon(label: string): string {
  return label.replace(/^\$\([^)]+\)\s*/, '');
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
