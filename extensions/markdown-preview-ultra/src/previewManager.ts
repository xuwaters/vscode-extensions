import * as path from 'path';
import * as vscode from 'vscode';
import { EngineBridge, EngineSession, type EngineOptions } from './engine';
import {
  isWebviewToHost,
  type HostToWebview,
  type PreviewSettings,
  type ToggleTaskMessage,
  type WebviewToHost,
} from './messages';
import { ParkedScroll, SyncGuard } from './scrollSync';
import { getNonce, isMarkdownDocument, isMarkdownPath } from './util';

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

/** Depth of the preview's own link history. */
const MAX_HISTORY = 50;

/** Verifies a line is a task-list item before the one-character toggle edit. */
const TASK_LINE = /^(\s*(?:[-*+]|\d+[.)])\s+)\[[ xX]\]/;

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
  private readonly engine: EngineBridge;
  private readonly disposables: vscode.Disposable[] = [];
  /** Ignore editor scroll events briefly after a preview-originated reveal. */
  private readonly editorScrollGuard = new SyncGuard();
  private readonly stateEmitter = new vscode.EventEmitter<void>();
  /** Fires when the preview opens, closes, or moves — modes/status bar. */
  public readonly onDidChangeState = this.stateEmitter.event;

  constructor(private readonly context: vscode.ExtensionContext) {
    this.engine = new EngineBridge(context.extensionUri.fsPath);
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
        const kind =
          theme.kind === vscode.ColorThemeKind.Light ||
          theme.kind === vscode.ColorThemeKind.HighContrastLight
            ? 'light'
            : 'dark';
        if (this.preview) {
          this.post(this.preview.panel.webview, { type: 'theme', kind });
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

  /** Move the existing panel to `column` (used by mode transitions). */
  public revealPanel(column: vscode.ViewColumn, preserveFocus = false): void {
    this.preview?.panel.reveal(column, preserveFocus);
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

  /** Open (or reveal + retarget) the preview for `document` in `viewColumn`. */
  public showPreview(
    document: vscode.TextDocument,
    viewColumn: vscode.ViewColumn,
  ): void {
    const existing = this.preview;
    if (existing) {
      this.retarget(existing, document);
      if (this.preview) {
        this.preview.panel.reveal(this.preview.panel.viewColumn, true);
      }
      return;
    }
    this.preview = this.createPreview(document, viewColumn);
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
      vscode.window.showInformationMessage('No preview is open to lock.');
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
  ): Preview {
    const resourceRoots = this.localResourceRoots(document);
    const toSide = viewColumn === vscode.ViewColumn.Beside;
    const lockGroup =
      toSide &&
      vscode.workspace
        .getConfiguration('markdownPreviewUltra')
        .get<boolean>('lockPreviewGroup', true);

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

    // Initial scroll alignment to the active editor for this document.
    const editor = vscode.window.visibleTextEditors.find(
      (ed) => ed.document.uri.toString() === document.uri.toString(),
    );
    if (editor) this.syncScroll(editor);

    return preview;
  }

  /** Wire a (created or deserialized) panel up as the live preview. */
  private attachPanel(
    panel: vscode.WebviewPanel,
    document: vscode.TextDocument,
  ): Preview {
    const sourceColumn =
      vscode.window.activeTextEditor?.viewColumn ?? vscode.ViewColumn.One;
    const resourceRoots = this.localResourceRoots(document);
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
    panel.webview.html = this.getHtml(panel.webview);

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

  /** Lock the preview's group so explorer/quick-open files open elsewhere. */
  private async lockGroup(): Promise<void> {
    // The panel just took focus, so its group is active. Lock it (a no-op if it
    // is already locked), then return focus to the source editor.
    await vscode.commands.executeCommand('workbench.action.lockEditorGroup');
    await vscode.commands.executeCommand('workbench.action.focusPreviousGroup');
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
    if (!this.withinRoots(preview.resourceRoots, document)) {
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
    this.retarget(preview, editor.document);
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
    this.post(preview.panel.webview, { type: 'visibility', visible });
    if (!visible) return;
    const line = preview.panelScroll.claim();
    if (line === undefined || !this.readSettings().scrollSync) return;
    this.post(preview.panel.webview, { type: 'scroll', line, ratio: 0 });
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
    if (line === undefined || !this.readSettings().scrollSync) return;
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
        // A restored panel can come up behind another tab; tell the page where
        // it stands before the first render so it knows not to measure.
        this.post(preview.panel.webview, {
          type: 'visibility',
          visible: preview.panel.visible,
        });
        this.update(preview);
        break;
      case 'revealLine':
        this.revealEditorLine(preview, msg.line, false);
        break;
      case 'jumpToLine':
        this.revealEditorLine(preview, msg.line, true);
        break;
      case 'navigate':
        void this.navigate(msg.direction);
        break;
      case 'openLink':
        void this.openLink(preview, msg.href);
        break;
      case 'toggleTask':
        void this.toggleTask(preview, msg);
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

    if (!preview.session) preview.session = this.engine.createSession();
    if (!preview.session) {
      this.post(panel.webview, { type: 'noEngine' });
      return;
    }

    const result = preview.session.render(
      document.getText(),
      this.readEngineOptions(),
    );
    if (!result) {
      this.post(panel.webview, { type: 'noEngine' });
      return;
    }

    this.post(panel.webview, {
      type: 'update',
      seq: result.seq,
      reset: result.reset,
      patches: result.patches,
      toc: result.toc,
      frontmatter: result.frontmatter,
      uri: document.uri.toString(),
      baseHref: this.baseHref(panel.webview, document),
      customStyles: this.customStyles(panel.webview, document),
      settings: this.readSettings(),
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
    if (!this.readSettings().scrollSync) return;
    const range = editor.visibleRanges[0];
    if (!range) return;
    if (!preview.panel.visible) {
      // Off-screen panel: park the line instead of scrolling a page that has
      // no layout to measure. Switching to the editor tab is itself a visible-
      // range change, so this is the path a tabbed Edit/Preview pair takes.
      preview.panelScroll.park(range.start.line);
      return;
    }
    this.post(preview.panel.webview, {
      type: 'scroll',
      line: range.start.line,
      ratio: 0,
    });
  }

  /** Preview-originated navigation → reveal (and optionally focus) editor. */
  private revealEditorLine(
    preview: Preview,
    line: number,
    focus: boolean,
  ): void {
    if (!focus && !this.readSettings().scrollSync) return;
    // Preview mode: the panel owns the source column, so there is nowhere to
    // put the editor except over the page being read. A double-click there
    // does nothing — reading is not a request to start editing.
    if (focus && preview.panel.viewColumn === preview.sourceColumn) return;
    this.editorScrollGuard.suppress();
    const target = new vscode.Range(line, 0, line, 0);
    const editor = vscode.window.visibleTextEditors.find(
      (ed) => ed.document.uri.toString() === preview.document.uri.toString(),
    );
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

  /** The one write path: flip `[ ]`/`[x]` after re-verifying the line. */
  private async toggleTask(
    preview: Preview,
    msg: ToggleTaskMessage,
  ): Promise<void> {
    const cfg = vscode.workspace.getConfiguration('markdownPreviewUltra');
    if (!cfg.get<boolean>('taskLists.toggleFromPreview', false)) return;
    const document = preview.document;
    if (msg.line >= document.lineCount) return;
    const line = document.lineAt(msg.line);
    const match = TASK_LINE.exec(line.text);
    if (!match) return;
    const checkboxChar = match[1].length + 1;
    const edit = new vscode.WorkspaceEdit();
    edit.replace(
      document.uri,
      new vscode.Range(msg.line, checkboxChar, msg.line, checkboxChar + 1),
      msg.checked ? 'x' : ' ',
    );
    await vscode.workspace.applyEdit(edit);
  }

  private async openLink(preview: Preview, href: string): Promise<void> {
    try {
      if (/^(https?|mailto):/i.test(href)) {
        await vscode.env.openExternal(vscode.Uri.parse(href));
        return;
      }
      const target = this.resolveLink(preview.document, href);
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

  /** Resolve a workspace-relative or document-relative link destination. */
  private resolveLink(document: vscode.TextDocument, href: string): vscode.Uri {
    // `other.md#section` names a file plus an anchor; only the file resolves.
    // Link destinations are percent-encoded (`my%20notes.md`) but URI paths
    // are held decoded, so undo that before joining.
    const raw = href.replace(/[#?].*$/, '');
    let file: string;
    try {
      file = decodeURIComponent(raw);
    } catch {
      file = raw;
    }
    const base = vscode.Uri.joinPath(document.uri, '..');
    return file.startsWith('/')
      ? vscode.Uri.joinPath(
          vscode.workspace.getWorkspaceFolder(document.uri)?.uri ?? base,
          file.replace(/^\/+/, ''),
        )
      : vscode.Uri.joinPath(base, file);
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

  // ── Settings ───────────────────────────────────────────────────────

  private readEngineOptions(): EngineOptions {
    const cfg = vscode.workspace.getConfiguration('markdownPreviewUltra');
    return {
      breaks: cfg.get<boolean>('breaks', false),
      linkify: cfg.get<boolean>('linkify', true),
      typographer: cfg.get<boolean>('typographer', false),
      html: cfg.get<boolean>('html.enabled', true),
      math: cfg.get<boolean>('math.enabled', true),
      mermaid: cfg.get<boolean>('mermaid.enabled', true),
      alerts: cfg.get<boolean>('alerts.enabled', true),
      emoji: cfg.get<boolean>('emoji.enabled', true),
      wikilinks: cfg.get<boolean>('wikiLinks.enabled', false),
    };
  }

  private readSettings(): PreviewSettings {
    const cfg = vscode.workspace.getConfiguration('markdownPreviewUltra');
    return {
      scrollSync: cfg.get<boolean>('scrollSync', true),
      math: cfg.get<boolean>('math.enabled', true),
      mermaid: cfg.get<boolean>('mermaid.enabled', true),
      mermaidTheme: cfg.get<PreviewSettings['mermaidTheme']>(
        'mermaid.theme',
        'auto',
      ),
      frontmatterDisplay: cfg.get<PreviewSettings['frontmatterDisplay']>(
        'frontmatter.display',
        'card',
      ),
      theme: cfg.get<PreviewSettings['theme']>('theme', 'github-light'),
      tocVisible: cfg.get<boolean>('toc.visible', false),
      tocWidth: cfg.get<number>('toc.width', 240),
      taskToggle: cfg.get<boolean>('taskLists.toggleFromPreview', false),
    };
  }

  // ── Webview plumbing ───────────────────────────────────────────────

  private baseHref(
    webview: vscode.Webview,
    document: vscode.TextDocument,
  ): string {
    const dir = vscode.Uri.joinPath(document.uri, '..');
    return webview.asWebviewUri(dir).toString().replace(/\/?$/, '/');
  }

  /** Resolve `customCss` (workspace-relative paths) to webview URIs. */
  private customStyles(
    webview: vscode.Webview,
    document: vscode.TextDocument,
  ): string[] {
    const files = vscode.workspace
      .getConfiguration('markdownPreviewUltra')
      .get<string[]>('customCss', []);
    if (files.length === 0) return [];
    const root =
      vscode.workspace.getWorkspaceFolder(document.uri)?.uri ??
      vscode.workspace.workspaceFolders?.[0]?.uri;
    if (!root) return [];
    return files.map((f) =>
      webview.asWebviewUri(vscode.Uri.joinPath(root, f)).toString(),
    );
  }

  private localResourceRoots(document: vscode.TextDocument): vscode.Uri[] {
    const roots: vscode.Uri[] = [
      vscode.Uri.joinPath(this.context.extensionUri, 'dist', 'webview'),
      vscode.Uri.joinPath(document.uri, '..'),
    ];
    for (const folder of vscode.workspace.workspaceFolders ?? []) {
      roots.push(folder.uri);
    }
    return roots;
  }

  /** Whether `document` lives under any granted resource root. */
  private withinRoots(
    roots: vscode.Uri[],
    document: vscode.TextDocument,
  ): boolean {
    const file = document.uri.toString();
    return roots.some((root) => {
      const r = root.toString().replace(/\/?$/, '/');
      return file === root.toString() || file.startsWith(r);
    });
  }

  private title(document: vscode.TextDocument, locked: boolean): string {
    const name = path.basename(document.uri.fsPath) || 'Untitled';
    return `${locked ? '🔒 ' : ''}Preview ${name}`;
  }

  private post(webview: vscode.Webview, message: HostToWebview): void {
    void webview.postMessage(message);
  }

  private getHtml(webview: vscode.Webview): string {
    const scriptUri = webview.asWebviewUri(
      vscode.Uri.joinPath(
        this.context.extensionUri,
        'dist',
        'webview',
        'index.js',
      ),
    );
    const styleUri = webview.asWebviewUri(
      vscode.Uri.joinPath(
        this.context.extensionUri,
        'dist',
        'webview',
        'style.css',
      ),
    );
    // Stamp a fixed theme onto <body> up front so the preview doesn't flash the
    // editor's colors before the first settings message reaches the webview.
    const theme = this.readSettings().theme;
    const bodyClass = theme === 'auto' ? '' : ` class="theme-${theme}"`;
    const nonce = getNonce();
    const csp = [
      `default-src 'none'`,
      `img-src ${webview.cspSource} https: data:`,
      `font-src ${webview.cspSource}`,
      `style-src ${webview.cspSource} 'unsafe-inline'`,
      `script-src 'nonce-${nonce}' ${webview.cspSource}`,
    ].join('; ');

    return /* html */ `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <meta http-equiv="Content-Security-Policy" content="${csp}">
  <link rel="stylesheet" href="${styleUri}">
  <title>Markdown Preview Ultra</title>
</head>
<body${bodyClass}>
  <div id="frontmatter"></div>
  <div id="content" class="markdown-preview"></div>
  <script nonce="${nonce}" type="module" src="${scriptUri}"></script>
</body>
</html>`;
  }
}

/** Whether the given document is the kind we can preview. */
export function canPreview(
  document: vscode.TextDocument | undefined,
): document is vscode.TextDocument {
  return !!document && isMarkdownDocument(document);
}
