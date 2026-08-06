import * as path from 'path';
import * as vscode from 'vscode';
import { applyTaskToggle, resolveLink } from './actions';
import type { EngineSession } from './engine';
import { isWebviewToHost, type WebviewToHost } from './messages';
import type { PreviewRenderer } from './renderer';
import { ParkedScroll, SyncGuard } from './scrollSync';
import { isMarkdownDocument, isMarkdownPath, visibleEditorFor } from './util';

/**
 * The single live preview panel. It follows the active markdown editor
 * (auto-retargeting on editor switch) unless `locked`, in which case it stays
 * pinned to its current document.
 */
interface Preview {
  readonly panel: vscode.WebviewPanel;
  document: vscode.TextDocument;
  /** Column the source editor lived in when opened — used by focus-toggle. */
  sourceColumn: vscode.ViewColumn;
  /** Pinned to its current file: stop following the active editor. */
  locked: boolean;
  /** Resource roots granted at creation; recreate if a new doc falls outside. */
  readonly resourceRoots: vscode.Uri[];
  /** Per-document WASM render session (block hashes for diffing). */
  session: EngineSession | null;
  /** Pending debounced re-render, if any. */
  debounce?: ReturnType<typeof setTimeout>;
  /** Previously-previewed document URIs, oldest first (browser-style back). */
  back: string[];
  /** Documents stepped back from, nearest first. */
  forward: string[];
  /** Whether the panel is the on-screen tab of its group (`panel.visible`). */
  visible: boolean;
  /**
   * Whether the page has announced itself and been rendered into. A panel is
   * `visible` from the moment it is created, but until this flips there is
   * nothing on the page to scroll — messages sent before it lands are dropped
   * by a script that is not listening yet.
   */
  ready: boolean;
  /** Editor position waiting for the panel to come back on screen. */
  panelScroll: ParkedScroll;
  /** Preview position waiting for the source editor to come forward. */
  editorScroll: ParkedScroll;
}

/** Serialized webview state used by the panel serializer across reloads. */
interface PanelState {
  uri?: string;
}

/** Context keys that drive the editor-title and preview-toolbar buttons. */
const CTX_VISIBLE = 'markdownPreviewUltra.previewVisible';
const CTX_LOCKED = 'markdownPreviewUltra.previewLocked';

const DEBOUNCE_MS = 150;

/**
 * The Edit-mode switch, contributed in `extension.ts`. The panel's own way out
 * of the preview is a whole-layout change — the panel closes and the source
 * takes the column back — which the mode manager owns; the toolbar's Edit
 * button asks for exactly what the editor title bar's Edit icon does.
 */
const SET_MODE_EDIT = 'markdownPreviewUltra.setModeEdit';

/** Depth of the preview's own link history. */
const MAX_HISTORY = 50;

/**
 * Owns the markdown preview. Markdown → HTML happens host-side in the WASM
 * engine; the webview receives block-level patches and applies DOM surgery.
 *
 * There is at most one preview. It follows whichever markdown editor is active
 * (so clicking a new `.md` file refreshes it) until the user locks it. When
 * opened to the side its editor group is locked so newly-opened files land in
 * the main group instead of clobbering the preview.
 */
export class PreviewManager implements vscode.Disposable {
  public static readonly viewType = 'markdownPreviewUltra.preview';

  private preview: Preview | undefined;
  private readonly disposables: vscode.Disposable[] = [];
  /** Ignore editor scroll events briefly after a preview-originated reveal. */
  private readonly editorScrollGuard = new SyncGuard();
  private readonly stateEmitter = new vscode.EventEmitter<void>();
  /** Fires when the preview opens, closes, or moves — modes/status bar. */
  public readonly onDidChangeState = this.stateEmitter.event;

  constructor(private readonly renderer: PreviewRenderer) {
    this.disposables.push(
      this.stateEmitter,
      vscode.workspace.onDidChangeTextDocument((e) =>
        this.onDocumentChanged(e.document),
      ),
      vscode.workspace.onDidChangeConfiguration((e) => {
        if (e.affectsConfiguration('markdownPreviewUltra')) {
          this.refresh();
        }
      }),
      vscode.window.onDidChangeActiveColorTheme((theme) => {
        if (this.preview) {
          this.renderer.postTheme(this.preview.panel.webview, theme);
        }
      }),
      // The light/dark switch belongs to the window: flipping it in a preview
      // tab restyles the panel too, without a re-render.
      this.renderer.onDidChangeThemeOverride(() => {
        if (this.preview) {
          this.renderer.postThemeOverride(this.preview.panel.webview);
        }
      }),
      vscode.window.onDidChangeTextEditorVisibleRanges((e) =>
        this.syncScroll(e.textEditor),
      ),
      // Follow the active editor: retarget the preview to the newly-active file.
      vscode.window.onDidChangeActiveTextEditor((editor) =>
        this.onActiveEditorChanged(editor),
      ),
    );
  }

  // ── Mode-manager surface ───────────────────────────────────────────

  public get hasPreview(): boolean {
    return this.preview !== undefined;
  }

  public get panelColumn(): vscode.ViewColumn | undefined {
    return this.preview?.panel.viewColumn ?? undefined;
  }

  public get sourceColumn(): vscode.ViewColumn | undefined {
    return this.preview?.sourceColumn;
  }

  /** The document a mode switch should act on. */
  public get currentDocument(): vscode.TextDocument | undefined {
    if (this.preview) return this.preview.document;
    const doc = vscode.window.activeTextEditor?.document;
    return doc && isMarkdownDocument(doc) ? doc : undefined;
  }

  /**
   * Move the existing panel to `column` (used by mode transitions). The lock
   * belongs to the editor group, not to the panel: a panel moving out to the
   * side lands in a group that has never been locked (the one it came from is
   * left behind, and empties out), so the lock has to be re-established there.
   */
  public async revealPanel(
    column: vscode.ViewColumn,
    preserveFocus = false,
  ): Promise<void> {
    const preview = this.preview;
    if (!preview) return;
    const lock = column !== preview.sourceColumn && this.lockGroupEnabled();
    // Locking acts on the *active* group, so when we intend to lock we must let
    // the panel take focus first, then hand it back.
    preview.panel.reveal(column, preserveFocus && !lock);
    if (lock) await this.lockGroup(preserveFocus);
    this.stateEmitter.fire();
  }

  /**
   * Close the preview, handing back the line it was last read to. In Preview
   * mode the source editor is a background tab that could not follow along, so
   * the caller has to place it there itself once it is back on screen.
   */
  public closePreview(): number | undefined {
    const line = this.preview?.editorScroll.claim();
    this.preview?.panel.dispose();
    return line;
  }

  // ── Commands ───────────────────────────────────────────────────────

  /**
   * Open (or reveal + retarget) the preview for `document` in `viewColumn`.
   *
   * `atLine` is the passage the reader is on, for callers that know it better
   * than the layout does: a mode switch has just moved the source editor, whose
   * visible range does not catch up until VSCode has laid it out again.
   */
  public showPreview(
    document: vscode.TextDocument,
    viewColumn: vscode.ViewColumn,
    atLine?: number,
  ): void {
    const existing = this.preview;
    if (existing) {
      this.retarget(existing, document);
      // `retarget` recreates the panel when the new file falls outside the
      // granted resource roots, so re-read it rather than reusing `existing`.
      const preview = this.preview;
      if (!preview) return;
      preview.panel.reveal(preview.panel.viewColumn, true);
      if (atLine !== undefined) this.scrollPanelTo(preview, atLine);
      return;
    }
    this.preview = this.createPreview(document, viewColumn, atLine);
    this.stateEmitter.fire();
  }

  /** Bounce focus between the source editor and its preview. */
  public toggleFocus(): void {
    const preview = this.preview;
    if (preview && preview.panel.active) {
      // Preview is focused → jump back to the source editor.
      void vscode.window.showTextDocument(preview.document, {
        viewColumn: preview.sourceColumn,
        preserveFocus: false,
      });
      return;
    }
    if (preview) {
      // Editor focused → jump into the preview.
      preview.panel.reveal(preview.panel.viewColumn, false);
      return;
    }
    // No preview yet → open one for the active markdown editor.
    const editor = vscode.window.activeTextEditor;
    if (canPreview(editor?.document)) {
      const mode = vscode.workspace
        .getConfiguration('markdownPreviewUltra')
        .get<string>('defaultMode', 'split');
      this.showPreview(
        editor.document,
        mode === 'preview' ? vscode.ViewColumn.Active : vscode.ViewColumn.Beside,
      );
    } else {
      vscode.window.showInformationMessage(
        'Open a Markdown file to show its preview.',
      );
    }
  }

  /**
   * Step the preview through its own link history. Following a markdown link
   * replaces what the panel shows, so the preview needs a way back that does
   * not depend on the editor's navigation stack.
   */
  public async navigate(direction: 'back' | 'forward'): Promise<void> {
    const preview = this.preview;
    if (!preview) return;
    const from = direction === 'back' ? preview.back : preview.forward;
    const to = direction === 'back' ? preview.forward : preview.back;
    const uri = from.pop();
    if (uri === undefined) return;
    let document: vscode.TextDocument;
    try {
      document = await vscode.workspace.openTextDocument(
        vscode.Uri.parse(uri, true),
      );
    } catch {
      // The file moved or was deleted: the entry is spent, stay put.
      this.update(preview);
      return;
    }
    to.push(preview.document.uri.toString());
    this.showSource(preview, document);
    this.retarget(preview, document, false);
  }

  /** Pin/unpin the preview to its current file (stop/resume following). */
  public togglePreviewLock(): void {
    const preview = this.preview;
    if (!preview) {
      vscode.window.showInformationMessage('No preview is open to pin.');
      return;
    }
    this.setLocked(preview, !preview.locked);
    vscode.window.showInformationMessage(
      preview.locked
        ? `Preview pinned to ${path.basename(preview.document.uri.fsPath)}.`
        : 'Preview will follow the active editor.',
    );
  }

  public dispose(): void {
    for (const d of this.disposables) d.dispose();
    this.preview?.panel.dispose();
    this.preview = undefined;
  }

  // ── Lifecycle ──────────────────────────────────────────────────────

  private createPreview(
    document: vscode.TextDocument,
    viewColumn: vscode.ViewColumn,
    atLine?: number,
  ): Preview {
    const resourceRoots = this.renderer.localResourceRoots(document);
    const toSide = viewColumn === vscode.ViewColumn.Beside;
    const lockGroup = toSide && this.lockGroupEnabled();

    const panel = vscode.window.createWebviewPanel(
      PreviewManager.viewType,
      this.title(document, false),
      // Locking a group acts on the *active* group, so when we intend to lock
      // we must let the new panel take focus, then hand it back below.
      { viewColumn, preserveFocus: !lockGroup },
      {
        enableScripts: true,
        enableFindWidget: true,
        retainContextWhenHidden: true,
        localResourceRoots: resourceRoots,
      },
    );

    const preview = this.attachPanel(panel, document);

    if (lockGroup) void this.lockGroup();

    // A preview opens on the passage being read, not at the top of the file:
    // on the line the caller hands over, or failing that wherever the source
    // editor is scrolled. The page has no content yet, so this parks — the
    // `ready` handler applies it once the first render has gone out.
    const line =
      atLine ?? visibleEditorFor(document)?.visibleRanges[0]?.start.line;
    if (line !== undefined) this.scrollPanelTo(preview, line);

    return preview;
  }

  /** Wire a (created or deserialized) panel up as the live preview. */
  private attachPanel(
    panel: vscode.WebviewPanel,
    document: vscode.TextDocument,
  ): Preview {
    const sourceColumn =
      vscode.window.activeTextEditor?.viewColumn ?? vscode.ViewColumn.One;
    const resourceRoots = this.renderer.localResourceRoots(document);
    const preview: Preview = {
      panel,
      document,
      sourceColumn,
      locked: false,
      resourceRoots,
      session: null,
      back: [],
      forward: [],
      visible: panel.visible,
      ready: false,
      panelScroll: new ParkedScroll(),
      editorScroll: new ParkedScroll(),
    };

    // A deserialized panel carries the roots it was serialized with, which need
    // not cover the document we are attaching. Restate them so the roots we
    // record are the roots actually enforced.
    panel.webview.options = {
      enableScripts: true,
      localResourceRoots: resourceRoots,
    };
    panel.webview.html = this.renderer.html(panel.webview);

    const onMessage = panel.webview.onDidReceiveMessage((msg: unknown) => {
      if (isWebviewToHost(msg)) this.onWebviewMessage(preview, msg);
    });
    const onViewState = panel.onDidChangeViewState(() => {
      this.onViewStateChanged(preview);
      this.stateEmitter.fire();
    });

    panel.onDidDispose(() => {
      if (preview.debounce) clearTimeout(preview.debounce);
      onMessage.dispose();
      onViewState.dispose();
      preview.session?.dispose();
      if (this.preview === preview) this.preview = undefined;
      void vscode.commands.executeCommand('setContext', CTX_VISIBLE, false);
      void vscode.commands.executeCommand('setContext', CTX_LOCKED, false);
      this.stateEmitter.fire();
    });

    void vscode.commands.executeCommand('setContext', CTX_VISIBLE, true);
    void vscode.commands.executeCommand('setContext', CTX_LOCKED, false);

    return preview;
  }

  /** Rebuild the preview from a serialized panel after a window reload. */
  public async restorePanel(
    panel: vscode.WebviewPanel,
    state: PanelState | undefined,
  ): Promise<void> {
    let document: vscode.TextDocument | undefined;
    if (state?.uri) {
      try {
        document = await vscode.workspace.openTextDocument(
          vscode.Uri.parse(state.uri, true),
        );
      } catch {
        document = undefined;
      }
    }
    if (!document) {
      const editor = vscode.window.activeTextEditor;
      if (canPreview(editor?.document)) document = editor.document;
    }
    if (!document) {
      panel.dispose();
      return;
    }
    this.preview?.panel.dispose();
    this.preview = this.attachPanel(panel, document);
    this.stateEmitter.fire();
  }

  /** Whether a side preview should lock the group it lands in. */
  private lockGroupEnabled(): boolean {
    return vscode.workspace
      .getConfiguration('markdownPreviewUltra')
      .get<boolean>('lockPreviewGroup', true);
  }

  /**
   * Lock the preview's group so explorer/quick-open files open elsewhere.
   * The panel must already hold focus: the command acts on the active group.
   * Callers that place the focus themselves afterwards pass `restoreFocus:
   * false` — awaiting this before they move on is what keeps the lock from
   * landing on whichever group they focus next.
   */
  private async lockGroup(restoreFocus = true): Promise<void> {
    // Lock the active group — a no-op if it is already locked.
    await vscode.commands.executeCommand('workbench.action.lockEditorGroup');
    if (restoreFocus) {
      await vscode.commands.executeCommand(
        'workbench.action.focusPreviousGroup',
      );
    }
  }

  /**
   * Point an existing preview at a different document, re-rendering it.
   * `record` appends the outgoing document to the back stack — history
   * navigation itself passes `false` so stepping back is not itself history.
   */
  private retarget(
    preview: Preview,
    document: vscode.TextDocument,
    record = true,
  ): void {
    if (preview.document.uri.toString() === document.uri.toString()) {
      preview.document = document;
      this.update(preview);
      return;
    }
    if (record) {
      preview.back.push(preview.document.uri.toString());
      if (preview.back.length > MAX_HISTORY) preview.back.shift();
      preview.forward.length = 0;
    }
    // Resource roots are fixed at creation; a file outside them would have its
    // images/links CSP-blocked, so recreate the panel in that case.
    if (!this.renderer.withinRoots(preview.resourceRoots, document)) {
      const wasLocked = preview.locked;
      const { back, forward } = preview;
      preview.panel.dispose();
      this.preview = this.createPreview(document, vscode.ViewColumn.Beside);
      // The panel is new; the reader's trail through the documents is not.
      this.preview.back = back;
      this.preview.forward = forward;
      if (wasLocked) this.setLocked(this.preview, true);
      return;
    }
    preview.document = document;
    preview.sourceColumn =
      vscode.window.activeTextEditor?.viewColumn ?? preview.sourceColumn;
    // A new document needs a fresh diff baseline, and lines parked against the
    // old one point nowhere.
    preview.session?.dispose();
    preview.session = null;
    preview.panelScroll.clear();
    preview.editorScroll.clear();
    this.update(preview);
    const editor = vscode.window.activeTextEditor;
    if (editor && editor.document.uri.toString() === document.uri.toString()) {
      this.syncScroll(editor);
    }
  }

  private setLocked(preview: Preview, locked: boolean): void {
    preview.locked = locked;
    preview.panel.title = this.title(preview.document, locked);
    void vscode.commands.executeCommand('setContext', CTX_LOCKED, locked);
  }

  // ── Events ─────────────────────────────────────────────────────────

  private onActiveEditorChanged(editor: vscode.TextEditor | undefined): void {
    const preview = this.preview;
    if (!preview) return;
    if (!editor || !isMarkdownDocument(editor.document)) return;
    if (preview.document.uri.toString() === editor.document.uri.toString()) {
      this.claimEditorScroll(preview, editor);
      return;
    }
    if (preview.locked) return;
    // Preview mode: the file that just opened landed in the panel's own group
    // and covered it. The panel owns that column, so retarget it to the new
    // file and bring it back in front — the editor stays behind as an inactive
    // tab. Only a *different* file does this: activating the previewed file's
    // own editor is how the reader asks to see the source.
    const inPanelColumn =
      editor.viewColumn !== undefined &&
      editor.viewColumn === preview.panel.viewColumn;
    this.retarget(preview, editor.document);
    // `retarget` recreates the panel when the new file falls outside the
    // granted resource roots, so re-read it rather than reusing `preview`.
    const panel = this.preview?.panel;
    if (inPanelColumn && panel && panel.viewColumn === editor.viewColumn) {
      panel.reveal(panel.viewColumn, false);
    }
  }

  /**
   * The panel came on screen or left it. A hidden panel is a webview without
   * layout — scrolling it lands against measurements that are all zero — so
   * the editor's position is parked while it is away and applied on return.
   */
  private onViewStateChanged(preview: Preview): void {
    const visible = preview.panel.visible;
    if (visible === preview.visible) return;
    preview.visible = visible;
    this.renderer.post(preview.panel.webview, { type: 'visibility', visible });
    // Before `ready` the page cannot act on either message; the line stays
    // parked for the `ready` handler, which runs with the panel on screen.
    if (visible && preview.ready) this.applyPanelScroll(preview);
  }

  /**
   * Put the page on `line`, or park it for whenever the page can be scrolled:
   * off screen it has no layout to measure against, and before `ready` it has
   * no content and nothing listening.
   */
  private scrollPanelTo(preview: Preview, line: number): void {
    if (!this.renderer.readSettings().scrollSync) return;
    if (!preview.ready || !preview.panel.visible) {
      preview.panelScroll.park(line);
      return;
    }
    this.renderer.post(preview.panel.webview, { type: 'scroll', line, ratio: 0 });
  }

  /** Hand the page the position that was waiting for it, if any. */
  private applyPanelScroll(preview: Preview): void {
    const line = preview.panelScroll.claim();
    if (line === undefined || !this.renderer.readSettings().scrollSync) return;
    this.renderer.post(preview.panel.webview, { type: 'scroll', line, ratio: 0 });
  }

  /**
   * The source editor came forward: adopt wherever the preview was read to
   * while the editor sat behind it as a background tab.
   */
  private claimEditorScroll(
    preview: Preview,
    editor: vscode.TextEditor,
  ): void {
    const line = preview.editorScroll.claim();
    if (line === undefined || !this.renderer.readSettings().scrollSync) return;
    this.editorScrollGuard.suppress();
    editor.revealRange(
      new vscode.Range(line, 0, line, 0),
      vscode.TextEditorRevealType.AtTop,
    );
    // Both sides now sit on this line, so the reveal events this triggers must
    // not park a position for the panel to jump to when it comes back.
    preview.panelScroll.clear();
  }

  private onWebviewMessage(preview: Preview, msg: WebviewToHost): void {
    switch (msg.type) {
      case 'ready':
        preview.ready = true;
        // A restored panel can come up behind another tab; tell the page where
        // it stands before the first render so it knows not to measure.
        this.renderer.post(preview.panel.webview, {
          type: 'visibility',
          visible: preview.panel.visible,
        });
        this.update(preview);
        // The patch above went out first, so the page has something to scroll
        // through by the time it reads this. A panel that came up behind
        // another tab leaves its line parked for `onViewStateChanged`.
        if (preview.panel.visible) this.applyPanelScroll(preview);
        break;
      case 'revealLine':
        this.revealEditorLine(preview, msg.line, false);
        break;
      case 'jumpToLine':
        this.revealEditorLine(preview, msg.line, true);
        break;
      case 'openSource':
        // The line the page was read to is handed over by `closePreview`, so
        // the editor lands on the passage rather than where it was parked.
        void vscode.commands.executeCommand(SET_MODE_EDIT);
        break;
      case 'navigate':
        void this.navigate(msg.direction);
        break;
      case 'openLink':
        void this.openLink(preview, msg.href);
        break;
      case 'setTheme':
        this.renderer.setThemeOverride(msg.theme);
        break;
      case 'toggleTask':
        void applyTaskToggle(preview.document, msg);
        break;
      case 'error':
        console.error(
          `markdown-preview-ultra webview error [${msg.context}]: ${msg.message}`,
        );
        // Escape hatch: rebuild from a clean baseline.
        this.forceReset(preview);
        break;
    }
  }

  private onDocumentChanged(document: vscode.TextDocument): void {
    const preview = this.preview;
    if (
      !preview ||
      preview.document.uri.toString() !== document.uri.toString()
    ) {
      return;
    }
    preview.document = document;
    if (preview.debounce) clearTimeout(preview.debounce);
    preview.debounce = setTimeout(() => {
      preview.debounce = undefined;
      this.update(preview);
    }, DEBOUNCE_MS);
  }

  private refresh(): void {
    // Engine options are compared WASM-side; a change forces `reset: true`.
    if (this.preview) this.update(this.preview);
  }

  private forceReset(preview: Preview): void {
    preview.session?.dispose();
    preview.session = null;
    this.update(preview);
  }

  // ── Rendering ──────────────────────────────────────────────────────

  /** Render through the engine and post the patch script to the webview. */
  private update(preview: Preview): void {
    const { document, panel } = preview;
    panel.title = this.title(document, preview.locked);
    preview.session ??= this.renderer.createSession();
    this.renderer.update(panel.webview, document, preview.session, {
      canGoBack: preview.back.length > 0,
      canGoForward: preview.forward.length > 0,
    });
  }

  private syncScroll(editor: vscode.TextEditor): void {
    const preview = this.preview;
    if (
      !preview ||
      preview.document.uri.toString() !== editor.document.uri.toString()
    ) {
      return;
    }
    if (this.editorScrollGuard.suppressed) return;
    const range = editor.visibleRanges[0];
    if (!range) return;
    // Off screen the line is parked rather than sent. Switching to the editor
    // tab is itself a visible-range change, so that is the path a tabbed
    // Edit/Preview pair takes.
    this.scrollPanelTo(preview, range.start.line);
  }

  /** Preview-originated navigation → reveal (and optionally focus) editor. */
  private revealEditorLine(
    preview: Preview,
    line: number,
    focus: boolean,
  ): void {
    if (!focus && !this.renderer.readSettings().scrollSync) return;
    // Preview mode: the panel owns the source column, so there is nowhere to
    // put the editor except over the page being read. A double-click there
    // does nothing — reading is not a request to start editing.
    if (focus && preview.panel.viewColumn === preview.sourceColumn) return;
    this.editorScrollGuard.suppress();
    const target = new vscode.Range(line, 0, line, 0);
    const editor = visibleEditorFor(preview.document);
    if (editor) {
      editor.revealRange(target, vscode.TextEditorRevealType.AtTop);
      if (focus) {
        void vscode.window.showTextDocument(preview.document, {
          viewColumn: editor.viewColumn,
          preserveFocus: false,
          selection: target,
        });
      }
      return;
    }
    if (!focus) {
      // Preview mode: the source is a background tab, which has no
      // `TextEditor` to reveal in. Park the line so the editor picks the
      // reader's place up when it comes forward.
      preview.editorScroll.park(line);
      return;
    }
    // Split mode with the source as a background tab: bring it forward in its
    // own column. Never move the panel — a jump to the source is navigation,
    // not a mode switch, and silently rearranging the user's layout out from
    // under a double-click is the wrong kind of helpful.
    void vscode.window.showTextDocument(preview.document, {
      viewColumn: preview.sourceColumn,
      preserveFocus: false,
      selection: target,
    });
  }

  private async openLink(preview: Preview, href: string): Promise<void> {
    try {
      if (/^(https?|mailto):/i.test(href)) {
        await vscode.env.openExternal(vscode.Uri.parse(href));
        return;
      }
      const target = resolveLink(preview.document, href);
      // Markdown links browse *in the preview*: opening an editor in the
      // active group would bury the panel the user is reading from. A pinned
      // preview stays on its file, so its links go to the editor instead.
      if (isMarkdownPath(target.fsPath) && !preview.locked) {
        const document = await vscode.workspace.openTextDocument(target);
        this.showSource(preview, document);
        this.retarget(preview, document);
        return;
      }
      await vscode.commands.executeCommand('vscode.open', target, {
        viewColumn: preview.sourceColumn,
      });
    } catch (err) {
      vscode.window.showErrorMessage(
        `Could not open link: ${err instanceof Error ? err.message : String(err)}`,
      );
    }
  }

  /**
   * Point the source editor at `document` — but only if one is already on
   * screen to point. In Preview mode the panel *is* the source column, and
   * opening an editor there would hide the preview.
   */
  private showSource(preview: Preview, document: vscode.TextDocument): void {
    const column = preview.sourceColumn;
    if (preview.panel.viewColumn === column) return;
    const inUse = vscode.window.visibleTextEditors.some(
      (ed) => ed.viewColumn === column,
    );
    if (!inUse) return;
    void vscode.window.showTextDocument(document, {
      viewColumn: column,
      preserveFocus: true,
    });
  }

  private title(document: vscode.TextDocument, locked: boolean): string {
    const name = path.basename(document.uri.fsPath) || 'Untitled';
    // A pin, not a padlock: the group lock VSCode draws on the tab bar is a
    // different thing, and two padlocks side by side read as one feature.
    return `${locked ? '📌 ' : ''}Preview ${name}`;
  }
}

/** Whether the given document is the kind we can preview. */
export function canPreview(
  document: vscode.TextDocument | undefined,
): document is vscode.TextDocument {
  return !!document && isMarkdownDocument(document);
}
