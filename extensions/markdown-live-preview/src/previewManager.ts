import * as path from 'path';
import * as vscode from 'vscode';
import type {
  HostToWebview,
  PreviewSettings,
  WebviewToHost,
} from './messages';
import { getNonce, isMarkdownDocument } from './util';

/** One live preview panel bound to a single source document. */
interface Preview {
  readonly panel: vscode.WebviewPanel;
  document: vscode.TextDocument;
  /** Pending debounced re-render, if any. */
  debounce?: ReturnType<typeof setTimeout>;
}

/**
 * Owns every markdown preview panel. The webview is a read-only renderer:
 * the host ships the full document text on each change and the webview
 * re-renders it. Previews are keyed by source URI so re-invoking the
 * command on the same file reveals the existing panel instead of stacking.
 */
export class PreviewManager implements vscode.Disposable {
  public static readonly viewType = 'markdownLivePreview.preview';

  private readonly previews = new Map<string, Preview>();
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
          this.refreshAll();
        }
      }),
      // Re-render on theme change so mermaid/highlight pick up the new colors.
      vscode.window.onDidChangeActiveColorTheme(() => this.refreshAll()),
      vscode.window.onDidChangeTextEditorVisibleRanges((e) =>
        this.syncScroll(e.textEditor),
      ),
    );
  }

  /** Open (or reveal) a preview for `document` in the given column. */
  public showPreview(
    document: vscode.TextDocument,
    viewColumn: vscode.ViewColumn,
  ): void {
    const key = document.uri.toString();
    const existing = this.previews.get(key);
    if (existing) {
      existing.document = document;
      existing.panel.reveal(viewColumn, true);
      this.update(existing);
      return;
    }

    const panel = vscode.window.createWebviewPanel(
      PreviewManager.viewType,
      this.title(document),
      { viewColumn, preserveFocus: true },
      {
        enableScripts: true,
        retainContextWhenHidden: true,
        localResourceRoots: this.localResourceRoots(document),
      },
    );

    const preview: Preview = { panel, document };
    this.previews.set(key, preview);

    panel.webview.html = this.getHtml(panel.webview);

    const onMessage = panel.webview.onDidReceiveMessage(
      (msg: WebviewToHost) => this.onWebviewMessage(preview, msg),
    );

    panel.onDidDispose(() => {
      if (preview.debounce) clearTimeout(preview.debounce);
      onMessage.dispose();
      this.previews.delete(key);
    });

    // Initial scroll alignment to the active editor for this document.
    const editor = vscode.window.visibleTextEditors.find(
      (ed) => ed.document.uri.toString() === key,
    );
    if (editor) this.syncScroll(editor);
  }

  public dispose(): void {
    for (const d of this.disposables) d.dispose();
    for (const preview of this.previews.values()) preview.panel.dispose();
    this.previews.clear();
  }

  // ── Internals ──────────────────────────────────────────────────────

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
    const preview = this.previews.get(document.uri.toString());
    if (!preview) return;
    preview.document = document;
    if (preview.debounce) clearTimeout(preview.debounce);
    preview.debounce = setTimeout(() => {
      preview.debounce = undefined;
      this.update(preview);
    }, 200);
  }

  private refreshAll(): void {
    for (const preview of this.previews.values()) this.update(preview);
  }

  /** Post the full document plus render settings to a preview's webview. */
  private update(preview: Preview): void {
    const { document, panel } = preview;
    panel.title = this.title(document);
    this.post(panel.webview, {
      type: 'update',
      markdown: document.getText(),
      fileName: path.basename(document.uri.fsPath) || 'Untitled',
      baseHref: this.baseHref(panel.webview, document),
      settings: this.readSettings(),
    });
  }

  private syncScroll(editor: vscode.TextEditor): void {
    const preview = this.previews.get(editor.document.uri.toString());
    if (!preview || !this.readSettings().scrollSync) return;
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

  private title(document: vscode.TextDocument): string {
    return `Preview ${path.basename(document.uri.fsPath) || 'Untitled'}`;
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
