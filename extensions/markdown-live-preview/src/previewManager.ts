import * as path from 'path';
import * as vscode from 'vscode';
import type {
  HostToWebview,
  PreviewSettings,
  WebviewToHost,
} from './messages';
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
  /** Pending debounced re-render, if any. */
  debounce?: ReturnType<typeof setTimeout>;
}

/** Context keys that drive the editor-title and preview-toolbar buttons. */
const CTX_VISIBLE = 'markdownLivePreview.previewVisible';
const CTX_LOCKED = 'markdownLivePreview.previewLocked';

/**
 * Owns the markdown preview. The webview is a read-only renderer: the host
 * ships the full document text on each change and the webview re-renders it.
 *
 * There is at most one preview. It follows whichever markdown editor is active
 * (so clicking a new `.md` file refreshes it) until the user locks it. When
 * opened to the side its editor group is locked so newly-opened files land in
 * the main group instead of clobbering the preview.
 */
export class PreviewManager implements vscode.Disposable {
  public static readonly viewType = 'markdownLivePreview.preview';

  private preview: Preview | undefined;
  private readonly disposables: vscode.Disposable[] = [];

  constructor(private readonly context: vscode.ExtensionContext) {
    this.disposables.push(
      vscode.workspace.onDidChangeTextDocument((e) =>
        this.onDocumentChanged(e.document),
      ),
      vscode.workspace.onDidChangeConfiguration((e) => {
        if (
          e.affectsConfiguration('markdownLivePreview') ||
          e.affectsConfiguration('editor')
        ) {
          this.refresh();
        }
      }),
      // Re-render on theme change so mermaid/highlight pick up the new colors.
      vscode.window.onDidChangeActiveColorTheme(() => this.refresh()),
      vscode.window.onDidChangeTextEditorVisibleRanges((e) =>
        this.syncScroll(e.textEditor),
      ),
      // Follow the active editor: retarget the preview to the newly-active file.
      vscode.window.onDidChangeActiveTextEditor((editor) =>
        this.onActiveEditorChanged(editor),
      ),
    );
  }

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
    // No preview yet → open one to the side for the active markdown editor.
    const editor = vscode.window.activeTextEditor;
    if (canPreview(editor?.document)) {
      this.showPreview(editor.document, vscode.ViewColumn.Beside);
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
    const sourceColumn =
      vscode.window.activeTextEditor?.viewColumn ?? vscode.ViewColumn.One;
    const resourceRoots = this.localResourceRoots(document);
    const toSide = viewColumn === vscode.ViewColumn.Beside;
    const lockGroup =
      toSide &&
      vscode.workspace
        .getConfiguration('markdownLivePreview')
        .get<boolean>('lockPreviewGroup', true);

    const panel = vscode.window.createWebviewPanel(
      PreviewManager.viewType,
      this.title(document, false),
      // Locking a group acts on the *active* group, so when we intend to lock
      // we must let the new panel take focus, then hand it back below.
      { viewColumn, preserveFocus: !lockGroup },
      {
        enableScripts: true,
        retainContextWhenHidden: true,
        localResourceRoots: resourceRoots,
      },
    );

    const preview: Preview = {
      panel,
      document,
      sourceColumn,
      locked: false,
      resourceRoots,
    };

    panel.webview.html = this.getHtml(panel.webview);

    const onMessage = panel.webview.onDidReceiveMessage(
      (msg: WebviewToHost) => this.onWebviewMessage(preview, msg),
    );

    panel.onDidDispose(() => {
      if (preview.debounce) clearTimeout(preview.debounce);
      onMessage.dispose();
      if (this.preview === preview) this.preview = undefined;
      void vscode.commands.executeCommand('setContext', CTX_VISIBLE, false);
      void vscode.commands.executeCommand('setContext', CTX_LOCKED, false);
    });

    void vscode.commands.executeCommand('setContext', CTX_VISIBLE, true);
    void vscode.commands.executeCommand('setContext', CTX_LOCKED, false);

    if (lockGroup) void this.lockGroup();

    // Initial scroll alignment to the active editor for this document.
    const editor = vscode.window.visibleTextEditors.find(
      (ed) => ed.document.uri.toString() === document.uri.toString(),
    );
    if (editor) this.syncScroll(editor);

    return preview;
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
      case 'openLink':
        void this.openLink(preview.document, msg.href);
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
    }, 200);
  }

  private refresh(): void {
    if (this.preview) this.update(this.preview);
  }

  // ── Rendering ──────────────────────────────────────────────────────

  /** Post the full document plus render settings to the preview's webview. */
  private update(preview: Preview): void {
    const { document, panel } = preview;
    panel.title = this.title(document, preview.locked);
    this.post(panel.webview, {
      type: 'update',
      markdown: document.getText(),
      fileName: path.basename(document.uri.fsPath) || 'Untitled',
      baseHref: this.baseHref(panel.webview, document),
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
    if (!this.readSettings().scrollSync) return;
    const range = editor.visibleRanges[0];
    if (!range) return;
    this.post(preview.panel.webview, { type: 'scroll', line: range.start.line });
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

  private readSettings(): PreviewSettings {
    const cfg = vscode.workspace.getConfiguration('markdownLivePreview');
    return {
      math: cfg.get<boolean>('math.enabled', true),
      mermaid: cfg.get<boolean>('mermaid.enabled', true),
      frontmatter: cfg.get<boolean>('frontmatter.enabled', true),
      breaks: cfg.get<boolean>('breaks', false),
      linkify: cfg.get<boolean>('linkify', true),
      scrollSync: cfg.get<boolean>('scrollSync', true),
    };
  }

  private baseHref(
    webview: vscode.Webview,
    document: vscode.TextDocument,
  ): string {
    const dir = vscode.Uri.joinPath(document.uri, '..');
    return webview.asWebviewUri(dir).toString().replace(/\/?$/, '/');
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
  <title>Markdown Preview</title>
</head>
<body>
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
