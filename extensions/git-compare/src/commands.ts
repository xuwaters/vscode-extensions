import * as vscode from 'vscode';
import { emptyUri } from './emptyProvider';
import type { GitAPI, Repository } from './gitApi';
import { isAddition, isDeletion, toGitUri } from './gitApi';
import { buildRefUri } from './refContentProvider';
import { pickRef, pickRepository } from './refPicker';
import { CompareState, type ComparisonSelection } from './state';
import type { CompareNode, CompareTreeDataProvider, FileNodePayload } from './treeProvider';
import type { FileEntry } from './fileTree';

export interface CommandDeps {
  api: GitAPI;
  state: CompareState;
  tree: CompareTreeDataProvider;
}

export function registerCommands(
  context: vscode.ExtensionContext,
  deps: CommandDeps,
): void {
  context.subscriptions.push(
    vscode.commands.registerCommand('gitCompare.choose', () => chooseComparison(deps)),
    vscode.commands.registerCommand('gitCompare.refresh', () => deps.tree.invalidate()),
    vscode.commands.registerCommand('gitCompare.clear', () => clearComparison(deps)),
    vscode.commands.registerCommand('gitCompare.openDiff', (arg) =>
      withPayload(arg, (p) => openDiff(p, deps)),
    ),
    vscode.commands.registerCommand('gitCompare.openAtRevision', (arg) =>
      withPayload(arg, (p) => openAtRevision(p, deps)),
    ),
    vscode.commands.registerCommand('gitCompare.openFile', (arg) =>
      withPayload(arg, (p) => openFile(p)),
    ),
  );
}

// view/item/context menu commands receive the element returned by
// TreeDataProvider.getChildren() — a CompareNode — not whatever we pass in
// `treeItem.command.arguments`. Click-on-row uses the latter. Accept both.
type Arg = CompareNode | FileNodePayload | undefined;

async function withPayload(arg: Arg, run: (p: FileNodePayload) => Promise<void>): Promise<void> {
  const payload = extractPayload(arg);
  if (!payload) {
    vscode.window.showWarningMessage('Git Compare: no file selected.');
    return;
  }
  await run(payload);
}

function extractPayload(arg: Arg): FileNodePayload | undefined {
  if (!arg) return undefined;
  if (isFileNodePayload(arg)) return arg;
  if ('kind' in arg && arg.kind === 'tree' && arg.node.kind === 'file') {
    return { entry: arg.node.entry, parent: arg.parent, group: arg.group };
  }
  return undefined;
}

function isFileNodePayload(arg: object): arg is FileNodePayload {
  return 'entry' in arg && 'parent' in arg && 'group' in arg;
}

async function chooseComparison(deps: CommandDeps): Promise<void> {
  const repo = await pickRepository(deps.api.repositories);
  if (!repo) {
    vscode.window.showWarningMessage('Git Compare: no Git repository in this workspace.');
    return;
  }
  const ref = await pickRef(repo);
  if (!ref) return;

  const selection: ComparisonSelection = {
    repoRoot: repo.rootUri.fsPath,
    ref: ref.ref,
    label: ref.label,
  };
  await deps.state.set(selection);
  await vscode.commands.executeCommand('setContext', 'gitCompare.hasComparison', true);
  await vscode.commands.executeCommand('gitCompare.view.focus');
}

async function clearComparison(deps: CommandDeps): Promise<void> {
  await deps.state.set(undefined);
  await vscode.commands.executeCommand('setContext', 'gitCompare.hasComparison', false);
}

async function openDiff(arg: FileNodePayload, deps: CommandDeps): Promise<void> {
  const { entry, parent, group } = arg;
  const repo = findRepo(deps.api, parent.repoRoot);
  if (!repo) return;

  const workingUri = entry.change.uri;
  const originalUri = entry.change.originalUri ?? entry.change.uri;
  const status = entry.change.status;

  if (group === 'changed') {
    // For added files the path doesn't exist at `ref`; for deleted files it
    // doesn't exist in the working tree. In either case, point one side at our
    // empty-content provider so the diff editor renders a clean "all added" /
    // "all removed" view rather than failing.
    const leftUri = isAddition(status)
      ? emptyUri(entry.relPath, `not in ${parent.label}`)
      : toGitUri(originalUri, parent.ref);
    const rightUri = isDeletion(status) ? emptyUri(entry.relPath, 'deleted') : workingUri;

    const title = buildDiffTitle(entry, parent, group);
    await vscode.commands.executeCommand('vscode.diff', leftUri, rightUri, title, {
      preview: true,
      preserveFocus: false,
    });
    return;
  }

  // For behind/ahead, diff the merge-base against either the compared ref
  // (behind) or HEAD (ahead) — that's the slice of history unique to that side.
  const mergeBase = await repo.getMergeBase('HEAD', parent.ref);
  if (!mergeBase) {
    vscode.window.showInformationMessage(
      `Git Compare: no common ancestor with ${parent.label}.`,
    );
    return;
  }
  const rightRef = group === 'behind' ? parent.ref : 'HEAD';
  const rightLabel = group === 'behind' ? parent.label : 'HEAD';
  const leftUri = isAddition(status)
    ? emptyUri(entry.relPath, 'not in common ancestor')
    : toGitUri(originalUri, mergeBase);
  const rightUri = isDeletion(status)
    ? emptyUri(entry.relPath, `not in ${rightLabel}`)
    : toGitUri(workingUri, rightRef);

  const title = buildDiffTitle(entry, parent, group);
  await vscode.commands.executeCommand('vscode.diff', leftUri, rightUri, title, {
    preview: true,
    preserveFocus: false,
  });
}

async function openAtRevision(arg: FileNodePayload, deps: CommandDeps): Promise<void> {
  const { entry, parent, group } = arg;
  const status = entry.change.status;

  // For "ahead" the right-hand revision is HEAD; otherwise it's the compared
  // ref. The user wants to see the file at whichever side of the comparison
  // contains it.
  const targetRef = group === 'ahead' ? 'HEAD' : parent.ref;
  const targetLabel = group === 'ahead' ? 'HEAD' : parent.label;

  // For "changed" the left side is the compared ref, so additions in the
  // working tree won't exist there. For behind/ahead, the right side is the
  // tip — deletions are the ones that won't exist at the tip.
  const missingAtTarget =
    group === 'changed' ? isAddition(status) : isDeletion(status);
  if (missingAtTarget) {
    vscode.window.showInformationMessage(
      `Git Compare: ${entry.relPath} does not exist at ${targetLabel}.`,
    );
    return;
  }
  // For "changed" the target is the *left* (older) side of the diff, so a
  // rename means the file lives at its original path. For behind/ahead the
  // target is the right (newer) side, where the renamed file is at its
  // current path.
  const useOriginal = group === 'changed';
  const sourceUri = useOriginal
    ? (entry.change.originalUri ?? entry.change.uri)
    : entry.change.uri;
  const sourceRel = useOriginal ? (entry.originalRelPath ?? entry.relPath) : entry.relPath;

  // Defensive: if the path really isn't reachable at the ref (e.g. a stale
  // status from a concurrent edit), fall back to a friendly message rather
  // than letting the git content provider's stderr bubble up to the user.
  const repo = findRepo(deps.api, parent.repoRoot);
  if (repo) {
    try {
      await repo.show(targetRef, sourceUri.fsPath);
    } catch {
      vscode.window.showInformationMessage(
        `Git Compare: ${entry.relPath} could not be read at ${targetLabel}.`,
      );
      return;
    }
  }

  // Probe the actual file URI first so VSCode tells us the languageId it
  // would have used for the working-tree file. Our custom scheme has no
  // file extension in the basename (the tab reads `utils.ts (main)`), so
  // VSCode can't infer the language on its own — we set it explicitly.
  const languageId = await detectLanguage(sourceUri);

  // Use our own scheme so the editor tab reads `<filename> (<ref>)`
  // rather than just the bare filename — gives the user immediate context
  // about which revision they're looking at. Open via openTextDocument +
  // showTextDocument so VSCode honors our custom-scheme URI's basename for
  // the tab title (vscode.open sometimes rewrites the label).
  const uri = buildRefUri({
    repoRoot: parent.repoRoot,
    ref: targetRef,
    filePath: sourceUri.fsPath,
    relPath: sourceRel,
    label: targetLabel,
  });
  const doc = await vscode.workspace.openTextDocument(uri);
  if (languageId && languageId !== doc.languageId) {
    await vscode.languages.setTextDocumentLanguage(doc, languageId);
  }
  await vscode.window.showTextDocument(doc, { preview: true });
}

async function detectLanguage(fileUri: vscode.Uri): Promise<string | undefined> {
  // openTextDocument on the working-tree file returns a doc whose languageId
  // VSCode picked using its own filename → language tables (extensions and
  // filename rules contributed by every installed language pack). It also
  // caches, so a subsequent open of the same file is cheap.
  try {
    const probe = await vscode.workspace.openTextDocument(fileUri);
    return probe.languageId;
  } catch {
    return undefined;
  }
}

async function openFile(arg: FileNodePayload): Promise<void> {
  const uri = arg.entry.change.uri;
  if (isDeletion(arg.entry.change.status)) {
    vscode.window.showInformationMessage(
      `Git Compare: ${arg.entry.relPath} has been deleted from the working tree.`,
    );
    return;
  }
  // `vscode.open` opens any file URI as a normal text editor — same behavior
  // as clicking the file in the Explorer.
  await vscode.commands.executeCommand('vscode.open', uri, { preview: false });
}

function findRepo(api: GitAPI, repoRoot: string): Repository | undefined {
  return api.repositories.find((r) => r.rootUri.fsPath === repoRoot);
}

function buildDiffTitle(
  entry: FileEntry,
  parent: ComparisonSelection,
  group: import('./treeProvider').GroupKind,
): string {
  const name = baseName(entry.relPath);
  if (group === 'changed') {
    if (entry.originalRelPath) {
      return `${baseName(entry.originalRelPath)} (${parent.label}) ↔ ${name}`;
    }
    return `${name} (${parent.label}) ↔ ${name}`;
  }
  const rightLabel = group === 'behind' ? parent.label : 'HEAD';
  return `${name} (merge-base) ↔ ${name} (${rightLabel})`;
}

function baseName(relPath: string): string {
  const idx = relPath.lastIndexOf('/');
  return idx === -1 ? relPath : relPath.slice(idx + 1);
}
