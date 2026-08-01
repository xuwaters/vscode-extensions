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
import { SyncGuard } from './scrollSync';
import { getNonce, isMarkdownDocument } from './util';

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
}

/** Serialized webview state used by the panel serializer across reloads. */
interface PanelState {
  uri?: string;
}

/** Context keys that drive the editor-title and preview-toolbar buttons. */
const CTX_VISIBLE = 'markdownPreviewUltra.previewVisible';
const CTX_LOCKED = 'markdownPreviewUltra.previewLocked';

const DEBOUNCE_MS = 150;

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

  public closePreview(): void {
    this.preview?.panel.dispose();
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
    const preview: Preview = {
      panel,
      document,
      sourceColumn,
      locked: false,
      resourceRoots: this.localResourceRoots(document),
      session: null,
    };

    panel.webview.html = this.getHtml(panel.webview);

    const onMessage = panel.webview.onDidReceiveMessage((msg: unknown) => {
      if (isWebviewToHost(msg)) this.onWebviewMessage(preview, msg);
    });
    const onViewState = panel.onDidChangeViewState(() =>
      this.stateEmitter.fire(),
    );

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

  /** Point an existing preview at a different document, re-rendering it. */
  private retarget(preview: Preview, document: vscode.TextDocument): void {
    if (preview.document.uri.toString() === document.uri.toString()) {
      preview.document = document;
      this.update(preview);
      return;
    }
    // Resource roots are fixed at creation; a file outside them would have its
    // images/links CSP-blocked, so recreate the panel in that case.
    if (!this.withinRoots(preview.resourceRoots, document)) {
      const wasLocked = preview.locked;
      preview.panel.dispose();
      this.preview = this.createPreview(document, vscode.ViewColumn.Beside);
      if (wasLocked) this.setLocked(this.preview, true);
      return;
    }
    preview.document = document;
    preview.sourceColumn =
      vscode.window.activeTextEditor?.viewColumn ?? preview.sourceColumn;
    // A new document needs a fresh diff baseline.
    preview.session?.dispose();
    preview.session = null;
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
    if (!preview || preview.locked) return;
    if (!editor || !isMarkdownDocument(editor.document)) return;
    if (preview.document.uri.toString() === editor.document.uri.toString()) {
      return;
    }
    this.retarget(preview, editor.document);
  }

  private onWebviewMessage(preview: Preview, msg: WebviewToHost): void {
    switch (msg.type) {
      case 'ready':
        this.update(preview);
        break;
      case 'revealLine':
        this.revealEditorLine(preview, msg.line, false);
        break;
      case 'jumpToLine':
        this.revealEditorLine(preview, msg.line, true);
        break;
      case 'openLink':
        void this.openLink(preview.document, msg.href);
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
    if (!focus) return;
    // No editor visible (Preview mode): make room, then open the source.
    if (preview.panel.viewColumn === preview.sourceColumn) {
      preview.panel.reveal(vscode.ViewColumn.Beside, true);
    }
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

  private async openLink(
    document: vscode.TextDocument,
    href: string,
  ): Promise<void> {
    try {
      if (/^(https?|mailto):/i.test(href)) {
        await vscode.env.openExternal(vscode.Uri.parse(href));
        return;
      }
      // Resolve a workspace-relative or document-relative link and open it.
      const base = vscode.Uri.joinPath(document.uri, '..');
      const target = href.startsWith('/')
        ? vscode.Uri.joinPath(
            vscode.workspace.getWorkspaceFolder(document.uri)?.uri ?? base,
            href.replace(/^\/+/, ''),
          )
        : vscode.Uri.joinPath(base, href);
      await vscode.commands.executeCommand('vscode.open', target);
    } catch (err) {
      vscode.window.showErrorMessage(
        `Could not open link: ${err instanceof Error ? err.message : String(err)}`,
      );
    }
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
      theme: cfg.get<PreviewSettings['theme']>('theme', 'auto'),
      tocVisible: cfg.get<boolean>('toc.visible', false),
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
<body>
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
