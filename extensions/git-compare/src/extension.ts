import * as vscode from 'vscode';
import { registerCommands } from './commands';
import { registerEmptyContentProvider } from './emptyProvider';
import { getGitApi, type GitAPI, type Repository } from './gitApi';
import { registerRefContentProvider } from './refContentProvider';
import { CompareState } from './state';
import { CompareTreeDataProvider } from './treeProvider';

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  const api = await getGitApi();
  if (!api) {
    await vscode.commands.executeCommand('setContext', 'gitCompare.hasRepo', false);
    return;
  }

  registerEmptyContentProvider(context);
  registerRefContentProvider(context, api);

  const state = new CompareState(context.workspaceState);
  context.subscriptions.push(state);

  const tree = new CompareTreeDataProvider(api, state);

  const view = vscode.window.createTreeView('gitCompare.view', {
    treeDataProvider: tree,
    showCollapseAll: true,
  });
  context.subscriptions.push(view);

  registerCommands(context, { api, state, tree });

  context.subscriptions.push(
    state.onDidChange(() => {
      tree.invalidate();
      void vscode.commands.executeCommand(
        'setContext',
        'gitCompare.hasComparison',
        state.get() !== undefined,
      );
    }),
  );

  await vscode.commands.executeCommand(
    'setContext',
    'gitCompare.hasComparison',
    state.get() !== undefined,
  );

  await syncHasRepoContext(api);
  context.subscriptions.push(
    api.onDidOpenRepository(() => {
      void syncHasRepoContext(api);
      tree.invalidate();
    }),
    api.onDidCloseRepository(() => {
      void syncHasRepoContext(api);
      tree.invalidate();
    }),
  );

  // Per-repository state listeners. Tracks new repos as they show up.
  const wired = new WeakSet<Repository>();
  const wireRepo = (repo: Repository): void => {
    if (wired.has(repo)) return;
    wired.add(repo);
    let timer: ReturnType<typeof setTimeout> | undefined;
    context.subscriptions.push(
      repo.state.onDidChange(() => {
        const sel = state.get();
        if (!sel || sel.repoRoot !== repo.rootUri.fsPath) return;
        if (timer) clearTimeout(timer);
        timer = setTimeout(() => tree.invalidate(), 250);
      }),
    );
  };
  for (const repo of api.repositories) wireRepo(repo);
  context.subscriptions.push(api.onDidOpenRepository((repo) => wireRepo(repo)));

  context.subscriptions.push(
    vscode.workspace.onDidSaveTextDocument(() => {
      const refresh = vscode.workspace
        .getConfiguration('gitCompare')
        .get<boolean>('refreshOnSave', true);
      if (refresh) tree.invalidate();
    }),
    vscode.workspace.onDidChangeConfiguration((e) => {
      if (e.affectsConfiguration('gitCompare.compactFolders')) {
        tree.invalidate();
      }
    }),
  );
}

async function syncHasRepoContext(api: GitAPI): Promise<void> {
  await vscode.commands.executeCommand(
    'setContext',
    'gitCompare.hasRepo',
    api.repositories.length > 0,
  );
}

export function deactivate(): void {}
