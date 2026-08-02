import * as vscode from 'vscode';
import { EngineBridge, EngineSession, type EngineOptions } from './engine';
import type { HostToWebview, PreviewSettings } from './messages';
import { getNonce } from './util';

/** Preview-local link history; drives the webview toolbar's ← / → buttons. */
export interface NavState {
  canGoBack: boolean;
  canGoForward: boolean;
}

/** A surface with no history of its own (the custom editor: one tab, one file). */
export const NO_HISTORY: NavState = { canGoBack: false, canGoForward: false };

/**
 * Everything the two preview surfaces — the following panel and the custom
 * editor — have in common: configuration reads, the webview document shell,
 * resource roots, and the render-and-post step. They differ in how they are
 * placed and what they follow; what they *show* is this.
 *
 * One instance is shared, so the WASM module is loaded at most once per window.
 */
export class PreviewRenderer {
  private readonly engine: EngineBridge;

  constructor(private readonly extensionUri: vscode.Uri) {
    this.engine = new EngineBridge(extensionUri.fsPath);
  }

  /** A fresh per-document render session (block hashes for diffing). */
  public createSession(): EngineSession | null {
    return this.engine.createSession();
  }

  public post(webview: vscode.Webview, message: HostToWebview): void {
    void webview.postMessage(message);
  }

  /** Forward a VSCode color-theme change to a page following the editor. */
  public postTheme(webview: vscode.Webview, theme: vscode.ColorTheme): void {
    const kind =
      theme.kind === vscode.ColorThemeKind.Light ||
      theme.kind === vscode.ColorThemeKind.HighContrastLight
        ? 'light'
        : 'dark';
    this.post(webview, { type: 'theme', kind });
  }

  /**
   * Render `document` through `session` and post the patch script. A missing
   * or failed engine shows the build hint instead of a blank page.
   */
  public update(
    webview: vscode.Webview,
    document: vscode.TextDocument,
    session: EngineSession | null,
    nav: NavState,
  ): void {
    if (!session) {
      this.post(webview, { type: 'noEngine' });
      return;
    }
    const result = session.render(document.getText(), this.readEngineOptions());
    if (!result) {
      this.post(webview, { type: 'noEngine' });
      return;
    }
    this.post(webview, {
      type: 'update',
      seq: result.seq,
      reset: result.reset,
      patches: result.patches,
      toc: result.toc,
      frontmatter: result.frontmatter,
      uri: document.uri.toString(),
      baseHref: this.baseHref(webview, document),
      customStyles: this.customStyles(webview, document),
      settings: this.readSettings(),
      canGoBack: nav.canGoBack,
      canGoForward: nav.canGoForward,
    });
  }

  // ── Settings ───────────────────────────────────────────────────────

  public readEngineOptions(): EngineOptions {
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

  public readSettings(): PreviewSettings {
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

  public baseHref(
    webview: vscode.Webview,
    document: vscode.TextDocument,
  ): string {
    const dir = vscode.Uri.joinPath(document.uri, '..');
    return webview.asWebviewUri(dir).toString().replace(/\/?$/, '/');
  }

  /** Resolve `customCss` (workspace-relative paths) to webview URIs. */
  public customStyles(
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

  public localResourceRoots(document: vscode.TextDocument): vscode.Uri[] {
    const roots: vscode.Uri[] = [
      vscode.Uri.joinPath(this.extensionUri, 'dist', 'webview'),
      vscode.Uri.joinPath(document.uri, '..'),
    ];
    for (const folder of vscode.workspace.workspaceFolders ?? []) {
      roots.push(folder.uri);
    }
    return roots;
  }

  /** Whether `document` lives under any granted resource root. */
  public withinRoots(
    roots: vscode.Uri[],
    document: vscode.TextDocument,
  ): boolean {
    const file = document.uri.toString();
    return roots.some((root) => {
      const r = root.toString().replace(/\/?$/, '/');
      return file === root.toString() || file.startsWith(r);
    });
  }

  public html(webview: vscode.Webview): string {
    const scriptUri = webview.asWebviewUri(
      vscode.Uri.joinPath(this.extensionUri, 'dist', 'webview', 'index.js'),
    );
    const styleUri = webview.asWebviewUri(
      vscode.Uri.joinPath(this.extensionUri, 'dist', 'webview', 'style.css'),
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
