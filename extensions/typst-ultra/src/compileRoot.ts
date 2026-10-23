import * as path from 'path';
import * as vscode from 'vscode';
import type { Client } from './lsp/client.js';
import * as config from './config.js';
import {
  documentSearchGlob,
  rankEntryCandidates,
  settingPath,
  templateEntrypoint,
  walkForDocuments,
  type DirectoryTree,
} from './entryPoints.js';

/**
 * Whether the compile root has been decided, as everything that has to respect
 * one sees it. Implemented by `CompileRoot`; an interface so the preview and
 * the export command depend on the question rather than on the status bar item.
 */
export interface RootState {
  /**
   * The file the server compiles, when that has been settled for it — a
   * session pin or `typstUltra.mainFile`. `undefined` means the root is
   * whatever the reader is looking at.
   *
   * Deliberately not just the pin: a project that checks its entry point into
   * `.vscode/settings.json` has answered the same question, and a caller that
   * only asked about the pin would compile the wrong file for it.
   */
  readonly entry: vscode.Uri | undefined;
}

/** What the preview needs on top of `RootState`: a way to *ask* about a root. */
export interface RootAdvisor extends RootState {
  suggestEntry(blank: vscode.Uri): Promise<void>;
}

/** Which of the three inputs decided the compile root. */
export type RootMode =
  | { kind: 'pinned'; uri: vscode.Uri }
  | { kind: 'setting'; uri: vscode.Uri }
  | { kind: 'following'; uri: vscode.Uri }
  | { kind: 'none' };

const PIN_KEY = 'typstUltra.mainFile';
const SUGGESTED_KEY = 'typstUltra.mainSuggested';

/**
 * How long a pause in typing means "that is the query".
 *
 * Long enough that walking a word back with backspace is one search rather
 * than six, short enough to feel like it is keeping up.
 */
const SEARCH_DELAY = 150;

/** Search hits to take. Past this the reader should type another letter. */
const SEARCH_LIMIT = 128;

/**
 * Context key: the settled main file's path, in a list, so a menu can ask
 * `resourcePath in typstUltra.mainFiles` of the file it was opened on.
 */
const MAIN_FILES_KEY = 'typstUltra.mainFiles';

/** Context key: whether a main file is settled at all, pinned or configured. */
const SETTLED_KEY = 'typstUltra.mainSettled';

/**
 * What marks the main file in the explorer and on its tab.
 *
 * Not a letter: those are git's, and `M` in particular already means
 * "modified". No colour either, for the same reason — the file name's colour
 * is how git says what state it is in, and that is worth more than ours.
 */
const MAIN_DECORATION = new vscode.FileDecoration('★', 'Typst main file');

/** A row of the main-file quick pick. */
interface PickItem extends vscode.QuickPickItem {
  action: 'unset' | 'pin' | 'share';
  uri?: vscode.Uri;
}

/** The row button that sets a file for everyone rather than for the reader. */
const SHARE_BUTTON: vscode.QuickInputButton = {
  iconPath: new vscode.ThemeIcon('save'),
  tooltip: 'Set as main file for everyone: save it to the workspace settings',
};

/** A file to pin, labelled the way the reader thinks of it. */
function fileItem(uri: vscode.Uri): PickItem {
  return {
    label: `$(file) ${vscode.workspace.asRelativePath(uri)}`,
    action: 'pin',
    uri,
    buttons: [SHARE_BUTTON],
  };
}

/** `vscode.workspace.fs`, in the shape the walk asks for. */
const WORKSPACE_TREE: DirectoryTree<vscode.Uri> = {
  async read(directory) {
    const entries = await vscode.workspace.fs.readDirectory(directory);
    return entries.map(
      ([name, type]) => [name, (type & vscode.FileType.Directory) !== 0] as const,
    );
  },
  join: (directory, name) => vscode.Uri.joinPath(directory, name),
  id: (directory) => directory.toString(),
};

/**
 * De-duplicate a pile of files, drop one of them, and put the likeliest entry
 * point first. Every list in this menu is built this way, from whichever
 * sources found it.
 */
function rank(found: readonly vscode.Uri[], exclude?: vscode.Uri): vscode.Uri[] {
  const skip = exclude?.toString();
  const byPath = new Map<string, vscode.Uri>();
  for (const uri of found) {
    if (uri.toString() === skip) continue;
    byPath.set(vscode.workspace.asRelativePath(uri), uri);
  }
  return rankEntryCandidates([...byPath.keys()]).map(
    (relative) => byPath.get(relative) as vscode.Uri,
  );
}

/**
 * The `.typ` files the reader has open, in tab order, nearest group first.
 *
 * Tabs rather than `visibleTextEditors`: a tab exists as soon as the window's
 * layout is restored, while the editor behind it is only built when VSCode gets
 * round to it — which, during a startup, can be after an extension has woken up
 * and asked what is open.
 */
export function openDocuments(): vscode.Uri[] {
  const open: vscode.Uri[] = [];
  for (const group of vscode.window.tabGroups.all) {
    for (const tab of group.tabs) {
      const input: unknown = tab.input;
      const uri =
        input instanceof vscode.TabInputText || input instanceof vscode.TabInputCustom
          ? input.uri
          : undefined;
      if (uri && /\.typc?$/i.test(uri.path)) open.push(uri);
    }
  }
  return open;
}

/**
 * The compile root, in all three of its modes.
 *
 * Following the focused editor is the zero-configuration default, and it is
 * right for single-file documents and for reading someone else's project. A
 * settled main file is right for a project of several files, where editing
 * `chapters/03.typ` or `data.typ` should still produce whole-document
 * diagnostics — and, since the preview follows this too, a whole document to
 * look at rather than the blank page a file of `#let` bindings compiles to.
 *
 * The entry file is the first of:
 *
 * 1. the session pin (`typstUltra.pinMain`, stored in `workspaceState`),
 * 2. `typstUltra.mainFile`, which a team can check in,
 * 3. the focused `.typ` editor.
 *
 * The reader calls the first two "the main file" — the Typst web app's word
 * for it, and the one that does not collide with `typstUltra.rootPath`, which
 * is the compile root in the `typst --root` sense.
 */
export class CompileRoot
  implements vscode.Disposable, vscode.FileDecorationProvider, RootAdvisor
{
  private readonly item: vscode.StatusBarItem;
  private readonly disposables: vscode.Disposable[] = [];
  private readonly decorationsChanged = new vscode.EventEmitter<vscode.Uri[]>();
  readonly onDidChangeFileDecorations = this.decorationsChanged.event;
  private mode: RootMode = { kind: 'none' };
  /** The file wearing the main-file badge, so the next change can take it off. */
  private decorated: vscode.Uri | undefined;
  /**
   * What the running server was last told, `null` for "follow the editor" and
   * `undefined` for nothing yet. Every `typst/setMain` recompiles, and a
   * settings change lands here several times over.
   */
  private sent: string | null | undefined;

  constructor(
    private readonly context: vscode.ExtensionContext,
    private readonly client: Client,
  ) {
    this.item = vscode.window.createStatusBarItem(
      'typstUltra.mainFile',
      vscode.StatusBarAlignment.Right,
      100,
    );
    this.item.name = 'Typst Main File';
    this.item.command = 'typstUltra.selectMain';
    this.disposables.push(this.item, this.decorationsChanged);

    this.disposables.push(
      vscode.commands.registerCommand('typstUltra.selectMain', () => this.pick()),
      vscode.window.registerFileDecorationProvider(this),
      // A new server starts from its initialization options, which make the
      // main file where it *starts* but not where it stays: the first chapter
      // opened would take over. Only `typst/setMain` makes it stick.
      client.onDidStart(() => {
        this.sent = undefined;
        this.sync();
      }),
      vscode.window.onDidChangeActiveTextEditor((editor) => this.follow(editor)),
      // `typstUltra.mainFile` can change without the focus moving — someone
      // edits `.vscode/settings.json`, or pulls a branch that adds it — and
      // the status bar has to say so at once, not at the next click.
      vscode.workspace.onDidChangeConfiguration((event) => {
        if (event.affectsConfiguration('typstUltra.mainFile')) {
          this.follow(vscode.window.activeTextEditor);
        }
      }),
    );

    this.follow(vscode.window.activeTextEditor);
  }

  /** The current mode, for the status bar and tests. */
  get current(): RootMode {
    return this.mode;
  }

  /** Whether a main file is pinned for this session. */
  get pinned(): vscode.Uri | undefined {
    const stored = this.context.workspaceState.get<string>(PIN_KEY);
    return stored ? vscode.Uri.file(stored) : undefined;
  }

  /**
   * The settled compile root: the session pin, else `typstUltra.mainFile`.
   *
   * Reads its inputs afresh rather than the mode `follow` last computed. A
   * setting can change without the focus moving, and a preview that acted on a
   * stale answer would go on compiling the wrong file until the reader
   * happened to click something.
   */
  get entry(): vscode.Uri | undefined {
    return (
      this.pinned ??
      this.configured(vscode.window.activeTextEditor?.document.uri)
    );
  }

  /** Pin a file as the main file, for this reader in this workspace. */
  async pin(uri: vscode.Uri): Promise<void> {
    await this.context.workspaceState.update(PIN_KEY, uri.fsPath);
    this.follow(vscode.window.activeTextEditor);
  }

  /**
   * Make a file the main file for everyone who opens the project, by writing
   * it to `typstUltra.mainFile` in the workspace settings.
   *
   * Clears the reader's own pin afterwards, since it outranks the setting: a
   * pin left behind would make the setting look like it had not taken.
   */
  async share(uri: vscode.Uri): Promise<void> {
    const folder = vscode.workspace.getWorkspaceFolder(uri);
    const root = config.resolveRoot(config.read(uri), uri);
    try {
      await vscode.workspace
        .getConfiguration('typstUltra', uri)
        .update(
          'mainFile',
          settingPath(root, uri.fsPath),
          folder
            ? vscode.ConfigurationTarget.WorkspaceFolder
            : vscode.ConfigurationTarget.Workspace,
        );
    } catch (error) {
      // No folder open means no workspace settings to write to.
      void vscode.window.showErrorMessage(
        `Typst: could not save the main file to the workspace settings. ${String(error)}`,
      );
      return;
    }
    await this.context.workspaceState.update(PIN_KEY, undefined);
    this.follow(vscode.window.activeTextEditor);
  }

  /**
   * Go back to following the focused editor: drop the pin, and the workspace's
   * `typstUltra.mainFile` with it.
   *
   * Clearing only the pin would leave a project that configured its main file
   * exactly where it was, and an Unset that does nothing is the one thing this
   * command must not be.
   */
  async unset(): Promise<void> {
    const scope = this.entry ?? vscode.window.activeTextEditor?.document.uri;
    await this.context.workspaceState.update(PIN_KEY, undefined);

    const settings = vscode.workspace.getConfiguration('typstUltra', scope);
    const set = settings.inspect<string>('mainFile');
    const folder = scope ? vscode.workspace.getWorkspaceFolder(scope) : undefined;
    if (set?.workspaceFolderValue && folder) {
      await settings.update('mainFile', undefined, vscode.ConfigurationTarget.WorkspaceFolder);
    }
    if (set?.workspaceValue) {
      await settings.update('mainFile', undefined, vscode.ConfigurationTarget.Workspace);
    }
    // One in the user settings is the reader's default for every project, so
    // it is overridden here rather than deleted from all of them.
    if (set?.globalValue) {
      await settings.update(
        'mainFile',
        '',
        folder
          ? vscode.ConfigurationTarget.WorkspaceFolder
          : vscode.ConfigurationTarget.Workspace,
      );
    }
    this.follow(vscode.window.activeTextEditor);
  }

  provideFileDecoration(uri: vscode.Uri): vscode.FileDecoration | undefined {
    return this.decorated?.toString() === uri.toString() ? MAIN_DECORATION : undefined;
  }

  /** Update after the active editor changed. */
  follow(editor: vscode.TextEditor | undefined): void {
    const pinned = this.pinned;
    if (pinned) {
      this.mode = { kind: 'pinned', uri: pinned };
      this.refresh();
      return;
    }

    const configured = this.configured(editor?.document.uri);
    if (configured) {
      this.mode = { kind: 'setting', uri: configured };
      this.refresh();
      return;
    }

    if (editor?.document.languageId === 'typst') {
      this.mode = { kind: 'following', uri: editor.document.uri };
      void this.suggestPinning(editor.document.uri);
    } else if (this.mode.kind !== 'following') {
      this.mode = { kind: 'none' };
    }
    this.refresh();
  }

  /**
   * Bring everything that shows the main file up to date: the status bar item,
   * the explorer badge, the menus' context keys, and the server.
   */
  refresh(): void {
    const settled =
      this.mode.kind === 'pinned' || this.mode.kind === 'setting'
        ? this.mode.uri
        : undefined;

    if (this.mode.kind === 'none') {
      this.item.hide();
    } else {
      const name = path.basename(this.mode.uri.fsPath);
      const icon = { pinned: 'pin', setting: 'settings-gear', following: 'eye' }[this.mode.kind];
      this.item.text = `$(${icon}) Main: ${name}`;
      this.item.tooltip = this.tooltip(this.mode);
      this.item.show();
    }

    if (settled?.toString() !== this.decorated?.toString()) {
      const changed = [this.decorated, settled].filter(
        (uri): uri is vscode.Uri => uri !== undefined,
      );
      this.decorated = settled;
      this.decorationsChanged.fire(changed);
    }

    void vscode.commands.executeCommand('setContext', SETTLED_KEY, settled !== undefined);
    void vscode.commands.executeCommand(
      'setContext',
      MAIN_FILES_KEY,
      settled ? [settled.fsPath] : [],
    );
    this.sync();
  }

  /**
   * Tell the server the settled main file, or that there is none.
   *
   * The setting needs this as much as the pin does. Without it the server only
   * *started* on the configured file, and the first chapter edited became the
   * document in its place.
   */
  private sync(): void {
    if (!this.client.running) return;
    const uri = this.entry?.toString() ?? null;
    if (uri === this.sent) return;
    this.sent = uri;
    this.client.notify('typst/setMain', { uri });
  }

  /** What the status bar item says on hover, with the way out in it. */
  private tooltip(mode: Exclude<RootMode, { kind: 'none' }>): vscode.MarkdownString {
    const file = vscode.workspace.asRelativePath(mode.uri);
    const tip = new vscode.MarkdownString(undefined, true);
    tip.isTrusted = { enabledCommands: ['typstUltra.selectMain', 'typstUltra.unpinMain'] };
    switch (mode.kind) {
      case 'pinned':
        tip.appendMarkdown(`**Typst main file:** \`${file}\`\n\n`);
        tip.appendMarkdown(
          'Set by you, for this workspace. Every file of the project compiles and previews as part of it.\n\n',
        );
        tip.appendMarkdown(
          '[Change…](command:typstUltra.selectMain) · [Unset](command:typstUltra.unpinMain)',
        );
        break;
      case 'setting':
        tip.appendMarkdown(`**Typst main file:** \`${file}\`\n\n`);
        tip.appendMarkdown(
          'From `typstUltra.mainFile` in the workspace settings. Every file of the project compiles and previews as part of it.\n\n',
        );
        tip.appendMarkdown(
          '[Change…](command:typstUltra.selectMain) · [Unset](command:typstUltra.unpinMain)',
        );
        break;
      case 'following':
        tip.appendMarkdown(`**Typst main file:** the focused editor, \`${file}\`\n\n`);
        tip.appendMarkdown(
          'For a document split over several files, set its main file, so editing any part still checks and previews the whole.\n\n',
        );
        tip.appendMarkdown('[Set a main file…](command:typstUltra.selectMain)');
        break;
    }
    return tip;
  }

  /**
   * The QuickPick behind the status bar item.
   *
   * Opens on what is already known — the focused file, the other open editors
   * — and fills in from a short walk of the folders around them. It does not
   * enumerate the workspace to open: a project of any size made that a wait,
   * and the answer is nearly always the file the reader is in or one beside
   * it. Typing hands the question to the workspace search instead, which is
   * both narrowed by the query and bound by `files.exclude` and
   * `search.exclude`, so `node_modules` and build output stay out of the way.
   */
  async pick(): Promise<void> {
    const active = this.focused();
    const picker = vscode.window.createQuickPick<PickItem>();
    picker.title = 'Typst: main file';
    picker.placeholder =
      'Which file is the document? Type to search; the save button sets it for everyone';

    const head = this.actions(active);
    // The rows that are on screen whatever the query is, so a search hit does
    // not offer the same file a second time further down.
    const shown = new Set(head.flatMap((item) => (item.uri ? [item.uri.toString()] : [])));
    let nearby: PickItem[] = [];
    let walking = true;
    let searching = false;
    // Only the latest query may write the list: a slow search for `ch` must
    // not land on top of the results for `chapter`.
    let query = 0;
    let debounce: ReturnType<typeof setTimeout> | undefined;
    let search: vscode.CancellationTokenSource | undefined;

    picker.items = head;
    picker.busy = true;
    picker.show();

    const settle = () => {
      picker.busy = walking || searching;
    };

    void this.nearbyCandidates(active).then(
      (found) => {
        nearby = found
          .filter((uri) => !shown.has(uri.toString()))
          .map((uri) => fileItem(uri));
        for (const item of nearby) if (item.uri) shown.add(item.uri.toString());
        walking = false;
        if (!searching) picker.items = [...head, ...nearby];
        settle();
      },
      () => {
        walking = false;
        settle();
      },
    );

    picker.onDidChangeValue((value) => {
      if (debounce) clearTimeout(debounce);
      search?.cancel();
      search = undefined;
      const typed = value.trim();
      const mine = ++query;

      if (!typed) {
        searching = false;
        picker.items = [...head, ...nearby];
        settle();
        return;
      }

      searching = true;
      settle();
      debounce = setTimeout(() => {
        const source = new vscode.CancellationTokenSource();
        search = source;
        void this.searchWorkspace(typed, source.token).then(
          (found) => {
            if (mine !== query) return;
            picker.items = [
              ...head,
              ...nearby,
              ...found
                .filter((uri) => !shown.has(uri.toString()))
                .map((uri) => fileItem(uri)),
            ];
            searching = false;
            settle();
          },
          () => {
            if (mine !== query) return;
            searching = false;
            settle();
          },
        );
      }, SEARCH_DELAY);
    });

    const choice = await new Promise<PickItem | undefined>((resolve) => {
      picker.onDidAccept(() => resolve(picker.selectedItems[0]));
      picker.onDidTriggerItemButton((event) => resolve({ ...event.item, action: 'share' }));
      picker.onDidHide(() => resolve(undefined));
    });

    if (debounce) clearTimeout(debounce);
    search?.cancel();
    picker.dispose();

    if (!choice) return;
    if (choice.action === 'unset') {
      await this.unset();
    } else if (choice.uri && choice.action === 'share') {
      await this.share(choice.uri);
    } else if (choice.uri) {
      await this.pin(choice.uri);
    }
  }

  /**
   * Offer to settle the compile root after a document compiled to nothing.
   *
   * A file of `#let` bindings or a template of `#show` rules is a perfectly
   * good `.typ` that produces no pages, and a preview of one is a blank tab
   * with no explanation. Where the `main.typ` offer below guesses from the file
   * *names* in the project, this fires on the symptom itself, so it reaches the
   * layouts no naming convention would have caught.
   *
   * Shares the one-shot counter with that offer: between the two of them a
   * reader should be asked about the compile root once.
   */
  async suggestEntry(blank: vscode.Uri): Promise<void> {
    if (this.entry) return;
    // An untitled buffer cannot be part of a larger document: nothing on disk
    // can import a file that has no path. So its blank preview means "you have
    // not typed anything yet", and offering to pin `main.typ` over the top of
    // a scratch document would answer a question nobody asked.
    if (blank.scheme !== 'file') return;
    if (this.context.workspaceState.get<boolean>(SUGGESTED_KEY)) return;

    const candidates = await this.candidates(blank);
    const best = candidates[0];
    if (!best) return;

    await this.context.workspaceState.update(SUGGESTED_KEY, true);

    const choice = await vscode.window.showInformationMessage(
      `Typst: ${path.basename(blank.fsPath)} compiles to no pages, so it looks like part of a larger document rather than one of its own. Make ${vscode.workspace.asRelativePath(best)} the main file and preview that instead?`,
      `Use ${path.basename(best.fsPath)}`,
      'Choose a file…',
    );
    if (choice === 'Choose a file…') {
      await this.pick();
    } else if (choice) {
      await this.pin(best);
    }
  }

  dispose(): void {
    for (const disposable of this.disposables) disposable.dispose();
  }

  /**
   * The file the reader is on, which is not always an editor.
   *
   * Falls back to the last `.typ` we were following, because the preview is a
   * webview and a custom editor: click into one and `activeTextEditor` goes
   * away, taking with it the one row of this menu that is always right.
   */
  private focused(): vscode.Uri | undefined {
    return (
      vscode.window.activeTextEditor?.document.uri ??
      (this.mode.kind === 'following' ? this.mode.uri : undefined)
    );
  }

  /** The rows that need nothing looked up, so the menu can open on them. */
  private actions(active: vscode.Uri | undefined): PickItem[] {
    const items: PickItem[] = [];
    const settled =
      this.mode.kind === 'pinned' || this.mode.kind === 'setting'
        ? this.mode.uri
        : undefined;
    if (settled) {
      items.push({
        label: '$(eye) Follow the focused editor',
        description: `Unset ${path.basename(settled.fsPath)} as the main file`,
        action: 'unset',
      });
    }
    if (this.mode.kind === 'pinned') {
      items.push({
        label: `$(save) Share ${path.basename(this.mode.uri.fsPath)} with the project`,
        description: 'Save it to the workspace settings, for everyone who opens it',
        action: 'share',
        uri: this.mode.uri,
      });
    }
    if (active && active.toString() !== settled?.toString()) {
      items.push({
        label: `$(pin) Set ${path.basename(active.fsPath)} as main file`,
        description: 'The file you are looking at',
        action: 'pin',
        uri: active,
        buttons: [SHARE_BUTTON],
      });
    }
    return items;
  }

  /**
   * What to offer before the reader has typed: the other open editors, and the
   * documents a short walk from the focused file.
   *
   * The walk starts at the focused file's own folder and then at the project
   * root, which between them cover both halves of the usual answer — the
   * chapter beside the one being edited, and the `main.typ` that includes it.
   */
  private async nearbyCandidates(active: vscode.Uri | undefined): Promise<vscode.Uri[]> {
    const roots: vscode.Uri[] = [];
    if (active) roots.push(vscode.Uri.joinPath(active, '..'));
    const folder = active
      ? vscode.workspace.getWorkspaceFolder(active)
      : vscode.workspace.workspaceFolders?.[0];
    if (folder) roots.push(folder.uri);

    const walked = await walkForDocuments(roots, WORKSPACE_TREE);
    return rank([...openDocuments(), ...walked], active);
  }

  /**
   * The workspace's answer to what the reader typed.
   *
   * `undefined` for the excludes rather than a glob of our own: that is what
   * asks the search service for the reader's `files.exclude` and
   * `search.exclude`, so this skips the same `node_modules`, `target` and
   * build output that quick open does, including whatever the project added.
   */
  private async searchWorkspace(
    query: string,
    token: vscode.CancellationToken,
  ): Promise<vscode.Uri[]> {
    const found = await vscode.workspace.findFiles(
      documentSearchGlob(query),
      undefined,
      SEARCH_LIMIT,
      token,
    );
    return rank(found, this.focused());
  }

  /**
   * Every `.typ` in the workspace, likeliest entry point first, optionally
   * without one file — the one that just failed to be a document.
   *
   * The full sweep, kept for the offer that follows a blank compile: nobody is
   * waiting on a menu there, and the guess it makes should see everything.
   */
  private async candidates(exclude?: vscode.Uri): Promise<vscode.Uri[]> {
    const found = await vscode.workspace.findFiles('**/*.{typ,typc}', undefined, 200);
    return rank(found, exclude);
  }

  /** `typstUltra.mainFile`, resolved against the project root. */
  private configured(scope: vscode.Uri | undefined): vscode.Uri | undefined {
    const settings = config.read(scope);
    if (!settings.host.mainFile) return undefined;
    const root = config.resolveRoot(settings, scope);
    const absolute = path.isAbsolute(settings.host.mainFile)
      ? settings.host.mainFile
      : path.join(root, settings.host.mainFile);
    return vscode.Uri.file(absolute);
  }

  /**
   * Offer to settle the main file once, the first time someone opens a chapter
   * in a project that clearly names its document.
   *
   * One-shot per workspace: an offer that comes back is nagging, and the
   * command and the status bar are both still there for anyone who declines.
   */
  private async suggestPinning(current: vscode.Uri): Promise<void> {
    if (this.context.workspaceState.get<boolean>(SUGGESTED_KEY)) return;

    const folder = vscode.workspace.getWorkspaceFolder(current);
    if (!folder) return;

    const main = await this.namedEntry(folder.uri);
    if (!main || main.fsPath === current.fsPath) return;

    await this.context.workspaceState.update(SUGGESTED_KEY, true);

    const name = path.basename(main.fsPath);
    const choice = await vscode.window.showInformationMessage(
      `This project's document looks like ${vscode.workspace.asRelativePath(main)}. Make it the main file, so editing any part of the project still checks and previews the whole?`,
      `Use ${name}`,
      'Not now',
    );
    if (choice === `Use ${name}`) await this.pin(main);
  }

  /**
   * The document a project names outright, if it does: a template package's
   * `typst.toml` entry point, else a `main.typ` at the top.
   */
  private async namedEntry(folder: vscode.Uri): Promise<vscode.Uri | undefined> {
    const named: vscode.Uri[] = [];
    try {
      const manifest = await vscode.workspace.fs.readFile(
        vscode.Uri.joinPath(folder, 'typst.toml'),
      );
      const hint = templateEntrypoint(new TextDecoder().decode(manifest));
      if (hint) named.push(vscode.Uri.joinPath(folder, hint));
    } catch {
      // No manifest, which is most projects.
    }
    named.push(vscode.Uri.joinPath(folder, 'main.typ'));

    for (const uri of named) {
      try {
        await vscode.workspace.fs.stat(uri);
        return uri;
      } catch {
        // Named but not there; try the next.
      }
    }
    return undefined;
  }
}
