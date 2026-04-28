import * as vscode from 'vscode';
import type { Ref, Repository } from './gitApi';
import { RefType } from './gitApi';

export interface RefPickResult {
  ref: string;
  label: string;
}

interface RefQuickPickItem extends vscode.QuickPickItem {
  ref?: string;
  enterCommitish?: boolean;
}

const DEFAULT_BRANCH_NAMES = new Set(['main', 'master', 'trunk', 'develop']);

export async function pickRef(repo: Repository): Promise<RefPickResult | undefined> {
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
  qp.items = [
    {
      label: '$(edit) Enter commit-ish…',
      description: 'Branch, tag, or commit SHA',
      enterCommitish: true,
    },
  ];

  const result = new Promise<RefPickResult | undefined>((resolve) => {
    let resolved = false;
    qp.onDidAccept(() => {
      const picked = qp.activeItems[0];
      if (!picked) return;
      resolved = true;
      qp.hide();
      if (picked.enterCommitish) {
        void promptCommitish().then(resolve);
        return;
      }
      if (picked.ref) {
        resolve({ ref: picked.ref, label: stripIcon(picked.label) });
        return;
      }
      resolve(undefined);
    });
    qp.onDidHide(() => {
      if (!resolved) resolve(undefined);
      qp.dispose();
    });
  });

  qp.show();

  try {
    const refs = await collectRefs(repo);
    qp.items = buildItems(refs, headName);
    qp.placeholder = 'Select a branch, tag, or commit to compare against';
  } catch (err) {
    qp.placeholder = `Failed to list refs: ${(err as Error).message}`;
  } finally {
    qp.busy = false;
  }

  return result;
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

function buildItems(refs: readonly Ref[], headName: string | undefined): RefQuickPickItem[] {
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
  items.push({
    label: '$(edit) Enter commit-ish…',
    description: 'Branch, tag, or commit SHA',
    enterCommitish: true,
  });
  return items;
}

async function promptCommitish(): Promise<RefPickResult | undefined> {
  const entered = await vscode.window.showInputBox({
    title: 'Compare with commit-ish',
    prompt: 'Branch name, tag, or commit SHA',
    validateInput: (v) => (v.trim() === '' ? 'Required' : undefined),
  });
  if (!entered) return undefined;
  const trimmed = entered.trim();
  return { ref: trimmed, label: trimmed };
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
