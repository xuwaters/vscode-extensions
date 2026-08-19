import * as path from 'path';
import * as vscode from 'vscode';
import type { Client } from '../lsp/client.js';
import type { RootAdvisor } from '../compileRoot.js';
import * as config from '../config.js';
import { decideFollow } from './follow.js';
import {
  isAllowedLink,
  parseWebviewMessage,
  type HostToWebview,
  type PreviewSettings,
  type WebviewToHost,
} from './messages.js';
import { html } from './html.js';
import type { PageMemory } from './pageMemory.js';
import { readPlace, writePlace } from './place.js';
import { compileNow, fetchMetrics, fetchPages, jumpFromClick } from './rpc.js';
import { SyncGuard } from './sync.js';

/** Context keys the editor-title buttons and keybindings are gated on. */
const CTX_VISIBLE = 'typstUltra.previewVisible';
const CTX_LOCKED = 'typstUltra.previewLocked';

/**
 * The preview panel.
 *
 * One panel that follows the active `.typ` editor, matching
 * markdown-preview-ultra's model. Multiple simultaneous previews are
 * deliberately not supported: two panels means two answers to "which document
 * is this", and every sync rule then needs a caveat.
 */
export class PreviewManager implements vscode.Disposable {
  public static readonly viewType = 'typstUltra.preview';

  private panel: vscode.WebviewPanel | undefined;
  private target: vscode.Uri | undefined;
  /** Column the source editor lived in when the panel opened. */
  private source: vscode.ViewColumn = vscode.ViewColumn.One;
  private locked = false;
  private seq = 0;
  /** The document the reader has already been placed in, by URI. */
  private placed: string | undefined;
  /**
   * Whether the page has announced itself. A panel exists from the moment it is
   * created, but nothing is listening in it until its script has loaded —
   * messages sent before that are dropped, so sending them is not just wasted,
   * it would mark work as done that never happened.
   */
  private ready = false;
  /** The last compile result the server reported, so an empty document can be
   * told apart from one that has not compiled yet. */
  private lastStatus: 'compiling' | 'ok' | 'error' | undefined;
  private readonly disposables: vscode.Disposable[] = [];
  private readonly guard = new SyncGuard();
  private readonly stateEmitter = new vscode.EventEmitter<void>();
  /** Fires when the preview opens, closes, moves, or retargets. */
  public readonly onDidChangeState = this.stateEmitter.event;

  constructor(
    private readonly context: vscode.ExtensionContext,
    private readonly client: Client,
    private readonly output: vscode.OutputChannel,
    private readonly pages: PageMemory,
    private readonly root: RootAdvisor,
  ) {
    this.disposables.push(
      this.stateEmitter,
      vscode.window.onDidChangeActiveTextEditor((editor) =>
        this.onActiveEditorChanged(editor),
      ),
      vscode.window.onDidChangeTextEditorSelection((event) =>
        this.onCursorMoved(event),
      ),
      vscode.workspace.onDidChangeConfiguration((event) => {
        if (event.affectsConfiguration('typstUltra.preview')) this.pushSettings();
      }),
    );

    // The server tells us when a compile finished; that is the moment to
    // re-measure and refresh whatever the viewport is showing.
    this.disposables.push(
      this.client.onNotification('typst/compileStatus', (params) => {
        this.onCompileStatus(params);
      }),
    );
  }

  /** Whether a preview panel exists and is visible. */
  get visible(): boolean {
    return this.panel?.visible ?? false;
  }

  // ── Mode-manager surface ───────────────────────────────────────────

  /** Whether a panel exists at all, on screen or behind another tab. */
  get hasPreview(): boolean {
    return this.panel !== undefined;
  }

  get panelColumn(): vscode.ViewColumn | undefined {
    return this.panel?.viewColumn ?? undefined;
  }

  get sourceColumn(): vscode.ViewColumn | undefined {
    return this.panel ? this.source : undefined;
  }

  /**
   * The document a mode switch or an export should act on.
   *
   * The open panel wins: with the focus inside it there is no active editor to
   * read, and a pinned panel is showing the file the reader means. A closed
   * panel's last subject is not an answer — by then the active editor is.
   */
  get currentUri(): vscode.Uri | undefined {
    if (this.panel && this.target) return this.target;
    const active = vscode.window.activeTextEditor?.document;
    return active?.languageId === 'typst' ? active.uri : undefined;
  }

  /**
   * The source file a mode switch should hand the reader back to.
   *
   * Not the same question as `currentUri` once a compile root is in play: the
   * panel shows `main.typ` while the reader is editing `data.typ`, and closing
   * the preview should leave them in the file they were editing rather than
   * moving them to an entry point they never opened.
   */
  get sourceUri(): vscode.Uri | undefined {
    const active = vscode.window.activeTextEditor?.document;
    if (active?.languageId === 'typst') return active.uri;
    return this.currentUri;
  }

  /**
   * Open, or reveal, the preview for a document.
   *
   * With a compile root settled, that is the document — asking for a preview
   * of `data.typ` in a project whose entry point is `main.typ` means "show me
   * the project", not "show me a blank page where a file of `#let` bindings
   * would have been".
   */
  async show(uri: vscode.Uri, column: vscode.ViewColumn): Promise<void> {
    const subject = this.root.entry ?? uri;
    await this.client.start(subject);
    const retargeted = this.target?.toString() !== subject.toString();
    this.target = subject;

    if (this.panel) {
      if (retargeted) this.adoptTarget(subject);
      this.panel.reveal(this.panel.viewColumn, true);
      await this.refreshMetrics();
      return;
    }

    this.source =
      vscode.window.activeTextEditor?.viewColumn ?? vscode.ViewColumn.One;
    const lockGroup =
      column === vscode.ViewColumn.Beside && this.lockGroupEnabled();

    const panel = vscode.window.createWebviewPanel(
      PreviewManager.viewType,
      this.title(subject),
      // Locking a group acts on the *active* group, so when we intend to lock
      // the new panel has to take focus first and hand it back below.
      { viewColumn: column, preserveFocus: !lockGroup },
      {
        enableScripts: true,
        retainContextWhenHidden: true,
        // Free, and it works because the SVG carries real `<text>` runs.
        enableFindWidget: true,
        localResourceRoots: [this.context.extensionUri],
      },
    );

    this.adopt(panel);
    compileNow(this.client, subject, this.root.entry !== undefined);
    if (lockGroup) await this.lockGroup();
    await this.refreshMetrics();
  }

  /**
   * Move the panel to `column`. The lock belongs to the editor group, not to
   * the panel: a panel moving out to the side lands in a group that has never
   * been locked, so the lock has to be re-established there.
   */
  async revealPanel(
    column: vscode.ViewColumn,
    preserveFocus = false,
  ): Promise<void> {
    const panel = this.panel;
    if (!panel) return;
    const lock = column !== this.source && this.lockGroupEnabled();
    panel.reveal(column, preserveFocus && !lock);
    if (lock) await this.lockGroup(preserveFocus);
    this.stateEmitter.fire();
  }

  /** Close the panel, handing back the page it was last read to. */
  closePreview(): number | undefined {
    const page = this.target ? this.pages.peek(this.target.toString()) : undefined;
    this.panel?.dispose();
    return page;
  }

  /** Bounce focus between the source editor and its preview. */
  async toggleFocus(): Promise<void> {
    const panel = this.panel;
    if (!panel) {
      const uri = this.currentUri;
      if (!uri) {
        void vscode.window.showInformationMessage(
          'Typst: open a .typ file to show its preview.',
        );
        return;
      }
      const mode = config.read(uri).host.preview.defaultMode;
      await this.show(
        uri,
        mode === 'preview' ? vscode.ViewColumn.Active : vscode.ViewColumn.Beside,
      );
      return;
    }
    if (panel.active) {
      await this.focusSource();
      return;
    }
    panel.reveal(panel.viewColumn, false);
  }

  /** Attach to a panel, whether newly created or restored by the serializer. */
  adopt(panel: vscode.WebviewPanel): void {
    this.panel = panel;
    this.ready = false;
    this.source =
      vscode.window.activeTextEditor?.viewColumn ?? this.source;
    panel.webview.html = html(panel.webview, this.context.extensionUri);

    panel.webview.onDidReceiveMessage((raw: unknown) => {
      const message = parseWebviewMessage(raw);
      if (!message) {
        this.output.appendLine(
          `preview: dropped a message that did not match any known shape`,
        );
        return;
      }
      void this.onMessage(message);
    });

    panel.onDidChangeViewState(() => this.stateEmitter.fire());

    panel.onDidDispose(() => {
      this.panel = undefined;
      void vscode.commands.executeCommand('setContext', CTX_VISIBLE, false);
      void vscode.commands.executeCommand('setContext', CTX_LOCKED, false);
      this.stateEmitter.fire();
    });

    void vscode.commands.executeCommand('setContext', CTX_VISIBLE, true);
    void vscode.commands.executeCommand('setContext', CTX_LOCKED, this.locked);
    this.stateEmitter.fire();
  }

  /** Point the preview at a different document. */
  retarget(uri: vscode.Uri): void {
    if (this.target?.toString() === uri.toString()) return;
    this.target = uri;
    this.adoptTarget(uri);
    void this.refreshMetrics();
    this.stateEmitter.fire();
  }

  /** Stop following the active editor. */
  toggleLock(): boolean {
    this.locked = !this.locked;
    if (this.panel && this.target) this.panel.title = this.title(this.target);
    void vscode.commands.executeCommand('setContext', CTX_LOCKED, this.locked);
    this.stateEmitter.fire();
    return this.locked;
  }

  /** Scroll the preview to wherever the cursor is. */
  async syncToCursor(): Promise<void> {
    const editor = vscode.window.activeTextEditor;
    if (!editor || !this.panel || !this.target) return;
    await this.sendCursor(editor, true);
  }

  /** Flip colour inversion without writing the setting. */
  toggleInvert(): void {
    this.post({ type: 'goToPage', page: -1 });
  }

  dispose(): void {
    for (const disposable of this.disposables) disposable.dispose();
    this.panel?.dispose();
  }

  // ── Events ─────────────────────────────────────────────────────────

  /**
   * Follow the reader to the file they just opened — unless the project has
   * already said which file is the document.
   *
   * Following is the whole point of one shared panel in a folder of standalone
   * documents: clicking a second `.typ` should show that document, not leave
   * the previous one on screen with the wrong source beside it. In a project it
   * is the opposite of what the reader wants, because only the entry point
   * compiles to pages; `decideFollow` holds the rule.
   */
  private onActiveEditorChanged(editor: vscode.TextEditor | undefined): void {
    if (!this.panel) return;
    if (editor?.document.languageId !== 'typst') return;

    // Tracked even when the subject does not move: with a compile root set the
    // reader edits `chapters/03.typ` while the panel shows `main.typ`, and a
    // click on a page still has to put its source back where they are working.
    this.source = editor.viewColumn ?? this.source;

    const entry = this.root.entry;
    const action = decideFollow({
      active: editor.document.uri.toString(),
      target: this.target?.toString(),
      entry: entry?.toString(),
      locked: this.locked,
    });
    if (action === 'stay') return;

    const next = action === 'showEntry' ? entry : editor.document.uri;
    if (!next) return;

    // Preview mode: the file that just opened landed in the panel's own column
    // and covered it. The panel owns that column, so bring it back in front
    // once it is showing the new file — the editor stays behind as a tab.
    const inPanelColumn =
      editor.viewColumn !== undefined &&
      editor.viewColumn === this.panel.viewColumn;

    this.retarget(next);

    if (inPanelColumn) this.panel.reveal(this.panel.viewColumn, false);
  }

  private async onMessage(message: WebviewToHost): Promise<void> {
    switch (message.type) {
      case 'ready':
        this.ready = true;
        // The first message carries the fit and zoom the reader last chose, so
        // a panel that has just opened is set up the way they left the last one
        // rather than reverting to the default fit.
        this.post({
          type: 'init',
          settings: previewSettings(this.target),
          restore: readPlace(this.context),
        });
        await this.refreshMetrics();
        break;

      case 'viewport':
        await this.renderPages(
          message.first,
          message.last,
          message.known,
          message.zoom,
        );
        break;

      case 'click':
        await this.jumpFromClick(message.page, message.xPt, message.yPt);
        break;

      case 'scrolled':
        if (this.target) this.pages.park(this.target.toString(), message.page);
        await this.onPreviewScrolled(message.page, message.yPt);
        break;

      case 'openLink':
        if (isAllowedLink(message.href)) {
          void vscode.env.openExternal(vscode.Uri.parse(message.href));
        } else {
          this.output.appendLine(`preview: refused to open ${message.href}`);
        }
        break;

      case 'export':
        // The in-page button and the title-bar icon are the same command, told
        // which document to act on rather than left to guess from the focus —
        // which is in the panel, and therefore names no editor at all.
        if (this.target) {
          await vscode.commands.executeCommand('typstUltra.export', this.target);
        }
        break;

      case 'openSource':
        await this.focusSource();
        break;

      case 'state':
        await writePlace(this.context, {
          zoom: message.zoom,
          fit: message.fit,
          inverted: message.inverted,
        });
        break;

      case 'error':
        this.output.appendLine(`preview: ${message.context}: ${message.message}`);
        break;
    }
  }

  private onCompileStatus(params: unknown): void {
    if (typeof params !== 'object' || params === null) return;
    const state = (params as { state?: string }).state;
    if (state !== 'compiling' && state !== 'ok' && state !== 'error') return;

    this.lastStatus = state;
    this.post({ type: 'status', state });
    if (state === 'ok') void this.refreshMetrics();
  }

  // ── Rendering ──────────────────────────────────────────────────────

  /** Ask the server how many pages there are and how big they are. */
  private async refreshMetrics(): Promise<void> {
    if (!this.panel || !this.target || !this.ready) return;

    const key = this.target.toString();
    const result = await fetchMetrics(this.client, this.target);
    if (!result) return;

    this.post({ type: 'metrics', seq: ++this.seq, uri: key, pages: result.pages });

    // A successful compile that produced no pages is a blank tab with nothing
    // to read and no error to explain it. Almost always that is a data or
    // template file being previewed on its own, so the question worth asking is
    // the compile root's. Gated on `ok` because a document that has not
    // compiled yet, or failed to, is empty for its own reasons.
    if (this.lastStatus === 'ok' && result.pages.length === 0) {
      void this.root.suggestEntry(this.target);
    }

    // A document the reader has seen before opens where they left it — *once*,
    // on the way in. Every compile refreshes the metrics, and placing the
    // reader again on each of them would snap the view to a page boundary on
    // every keystroke. The webview resets its scroll when the URI changes, so
    // this has to follow the metrics that carry the new one.
    if (this.placed === key) return;
    this.placed = key;
    const page = this.pages.peek(key);
    if (page !== undefined && page > 0) this.post({ type: 'goToPage', page });
  }

  /** Fetch the pages in view that the webview does not already hold. */
  private async renderPages(
    first: number,
    last: number,
    known: Record<number, string>,
    zoom: number,
  ): Promise<void> {
    if (!this.target) return;

    const result = await fetchPages(
      this.client,
      this.target,
      first,
      last,
      known,
      zoom,
    );
    if (!result) return;

    this.post({ type: 'pages', seq: ++this.seq, patches: result.patches });
  }

  /** Preview → editor. */
  private async jumpFromClick(
    page: number,
    xPt: number,
    yPt: number,
  ): Promise<void> {
    const result = await jumpFromClick(this.client, page, xPt, yPt);
    if (!result) return;

    if (result.kind === 'url') {
      if (isAllowedLink(result.url)) {
        void vscode.env.openExternal(vscode.Uri.parse(result.url));
      }
      return;
    }
    if (result.kind === 'page') {
      this.post({ type: 'goToPage', page: result.page });
      return;
    }

    const uri = vscode.Uri.parse(result.uri);
    const document = await vscode.workspace.openTextDocument(uri);
    const editor = await vscode.window.showTextDocument(document, {
      preserveFocus: false,
      // The source belongs where the source has been all along. Opening it in
      // the active column would put it over the page that was just clicked.
      viewColumn: this.source,
    });

    const position = new vscode.Position(
      result.position.line,
      result.position.character,
    );
    this.guard.markPreviewOrigin();
    editor.selection = new vscode.Selection(position, position);
    editor.revealRange(
      new vscode.Range(position, position),
      vscode.TextEditorRevealType.InCenterIfOutsideViewport,
    );
  }

  /** Preview scroll → editor scroll. */
  private async onPreviewScrolled(page: number, yPt: number): Promise<void> {
    const settings = config.read(this.target);
    const mode = settings.host.preview.scrollSync;
    if (mode !== 'both' && mode !== 'previewToEditor') return;
    if (this.guard.isEditorOrigin()) return;

    const result = await jumpFromClick(this.client, page, 20, yPt);
    if (!result || result.kind !== 'source') return;

    const editor = vscode.window.visibleTextEditors.find(
      (candidate) => candidate.document.uri.toString() === result.uri,
    );
    if (!editor) return;

    this.guard.markPreviewOrigin();
    const position = new vscode.Position(result.position.line, 0);
    editor.revealRange(
      new vscode.Range(position, position),
      vscode.TextEditorRevealType.AtTop,
    );
  }

  /** Editor cursor → preview. */
  private onCursorMoved(event: vscode.TextEditorSelectionChangeEvent): void {
    if (!this.panel || event.textEditor.document.languageId !== 'typst') return;

    const settings = config.read(event.textEditor.document.uri);
    const mode = settings.host.preview.scrollSync;
    if (mode !== 'both' && mode !== 'editorToPreview') return;
    if (this.guard.isPreviewOrigin()) return;

    this.guard.debounce(50, () => void this.sendCursor(event.textEditor, false));
  }

  private async sendCursor(
    editor: vscode.TextEditor,
    force: boolean,
  ): Promise<void> {
    if (!this.panel) return;

    const settings = config.read(editor.document.uri);
    if (!force && !settings.host.preview.cursorIndicator) return;

    const position = editor.selection.active;
    const points = await this.client.request<
      { page: number; xPt: number; yPt: number }[]
    >('typst/jumpFromCursor', {
      uri: editor.document.uri.toString(),
      position: { line: position.line, character: position.character },
    });

    // `jump_from_cursor` returns nothing for a cursor in a comment or on a
    // keyword — leave the preview where it is rather than jumping somewhere
    // arbitrary.
    const first = points?.[0];
    if (!first) return;

    this.guard.markEditorOrigin();
    this.post({
      type: 'cursor',
      page: first.page,
      xPt: first.xPt,
      yPt: first.yPt,
    });
  }

  // ── Layout ─────────────────────────────────────────────────────────

  /** Put the source of the previewed document back in front of the reader. */
  private async focusSource(): Promise<void> {
    const uri = this.target;
    if (!uri) return;
    const document = await vscode.workspace.openTextDocument(uri);
    await vscode.window.showTextDocument(document, {
      viewColumn: this.source,
      preserveFocus: false,
    });
  }

  /** Whether a side preview should lock the group it lands in. */
  private lockGroupEnabled(): boolean {
    return config.read(this.target).host.preview.lockPreviewGroup;
  }

  /**
   * Lock the preview's group so explorer and quick-open files land in the main
   * group instead of replacing the preview. The command acts on the *active*
   * group, so the panel must already hold focus; callers that place the focus
   * themselves afterwards pass `restoreFocus: false`.
   */
  private async lockGroup(restoreFocus = true): Promise<void> {
    await vscode.commands.executeCommand('workbench.action.lockEditorGroup');
    if (restoreFocus) {
      await vscode.commands.executeCommand(
        'workbench.action.focusPreviousGroup',
      );
    }
  }

  /** Everything a change of subject implies, short of asking for the pages. */
  private adoptTarget(uri: vscode.Uri): void {
    if (this.panel) this.panel.title = this.title(uri);
    compileNow(this.client, uri, this.root.entry !== undefined);
  }

  private pushSettings(): void {
    this.post({ type: 'settings', settings: previewSettings(this.target) });
  }

  private post(message: HostToWebview): void {
    void this.panel?.webview.postMessage(message);
  }

  private title(uri: vscode.Uri | undefined): string {
    // A pin, not a padlock: the group lock VSCode draws on the tab bar is a
    // different thing, and two padlocks side by side read as one feature.
    // Codicons do not render in a panel title, so this is a real character.
    return `${this.locked ? '📌 ' : ''}Preview ${uriLabel(uri)}`;
  }
}

/** The subset of the configuration the webview acts on. */
export function previewSettings(scope?: vscode.Uri): PreviewSettings {
  const settings = config.read(scope).host.preview;
  return {
    scrollSync: settings.scrollSync,
    cursorIndicator: settings.cursorIndicator,
    invertColors: settings.invertColors,
    background: settings.background,
    renderMode: settings.renderMode,
  };
}

function uriLabel(uri: vscode.Uri | undefined): string {
  return uri ? path.basename(uri.fsPath) || 'Typst' : 'Typst';
}
