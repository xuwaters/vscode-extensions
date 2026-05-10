import * as vscode from 'vscode';
import { getNonce } from './util.js';
import type {
  FilterRule,
  HostToWebview,
  ParsedLines,
  ViewState,
  WasmModule,
  WebviewToHost,
} from './types.js';

interface PanelEntry {
  panel: vscode.WebviewPanel;
  document: vscode.TextDocument;
  state: ViewState;
  rules: FilterRule[];
  reparse: () => void;
  rematch: () => void;
}

export class LogEditorProvider implements vscode.CustomTextEditorProvider {
  public static readonly viewType = 'logViewer.editor';

  private readonly entries = new Set<PanelEntry>();
  private activeEntry: PanelEntry | undefined;

  constructor(
    private readonly context: vscode.ExtensionContext,
    private readonly wasm: WasmModule | null,
  ) {}

  public async resolveCustomTextEditor(
    document: vscode.TextDocument,
    webviewPanel: vscode.WebviewPanel,
    _token: vscode.CancellationToken,
  ): Promise<void> {
    const webview = webviewPanel.webview;
    webview.options = { enableScripts: true };
    webview.html = this.getHtmlForWebview(webview);

    if (!this.wasm) {
      webview.html = this.errorHtml(
        'log-parser WASM bundle is missing. Build it with <code>pnpm run build:wasm</code> in <code>extensions/log-viewer</code>.',
      );
      return;
    }
    const wasm = this.wasm;

    let lines: ParsedLines = { html: [], text: [] };
    let filterMatches: number[] = [];
    let truncated = false;
    let totalBytes = 0;

    const state: ViewState = this.readViewState();
    const rules: FilterRule[] = this.readRules();

    const reparse = (): void => {
      const text = document.getText();
      totalBytes = Buffer.byteLength(text, 'utf8');
      const max = vscode.workspace
        .getConfiguration('logViewer')
        .get<number>('maxFileSizeBytes', 8 * 1024 * 1024);
      truncated = totalBytes > max;
      const slice = truncated ? text.slice(0, Math.floor(max / 2)) : text;
      const idx = new wasm.LogIndex(slice);
      try {
        lines = JSON.parse(idx.allLinesJson()) as ParsedLines;
      } finally {
        idx.free();
      }
      rematch();
    };

    const rematch = (): void => {
      if (lines.text.length === 0) {
        filterMatches = [];
        return;
      }
      // Build a fresh LogIndex over the plain text to leverage the matchFilters
      // path. (We could also implement matching in JS — keeping it in Rust
      // keeps regex semantics consistent.) The plain-text input avoids
      // re-parsing ANSI a second time.
      const idx = new wasm.LogIndex(lines.text.join('\n'));
      try {
        const bytes = idx.matchFilters(JSON.stringify(rules));
        filterMatches = Array.from(bytes);
      } finally {
        idx.free();
      }
    };

    const sendInit = (): void => {
      this.post(webview, {
        type: 'init',
        lines,
        filterMatches,
        rules,
        state,
        truncated,
        totalBytes,
      });
    };

    const sendUpdate = (
      payload: Omit<Extract<HostToWebview, { type: 'update' }>, 'type'>,
    ): void => {
      this.post(webview, { type: 'update', ...payload });
    };

    const entry: PanelEntry = {
      panel: webviewPanel,
      document,
      state,
      rules,
      reparse,
      rematch,
    };
    this.entries.add(entry);
    if (webviewPanel.active) this.activeEntry = entry;

    reparse();

    const onMessage = webview.onDidReceiveMessage((msg: WebviewToHost) => {
      switch (msg.type) {
        case 'ready':
          sendInit();
          break;
        case 'setState':
          Object.assign(state, msg.state);
          sendUpdate({ state });
          break;
        case 'setFilterEnabled': {
          const r = rules[msg.index];
          if (!r) break;
          r.enabled = msg.enabled;
          rematch();
          sendUpdate({ rules, filterMatches });
          break;
        }
        case 'openInText':
          void vscode.commands.executeCommand(
            'vscode.openWith',
            document.uri,
            'default',
          );
          break;
      }
    });

    const onDocChange = vscode.workspace.onDidChangeTextDocument((e) => {
      if (e.document.uri.toString() !== document.uri.toString()) return;
      reparse();
      sendUpdate({ lines, filterMatches, truncated, totalBytes });
    });

    const onConfigChange = vscode.workspace.onDidChangeConfiguration((e) => {
      if (!e.affectsConfiguration('logViewer')) return;
      const next = this.readViewState();
      Object.assign(state, next);
      const newRules = this.readRules();
      // Preserve session toggles when the rule list is unchanged in shape.
      const sessionEnabled = new Map(rules.map((r) => [r.name, r.enabled]));
      for (const r of newRules) {
        if (sessionEnabled.has(r.name)) r.enabled = sessionEnabled.get(r.name);
      }
      rules.length = 0;
      rules.push(...newRules);
      rematch();
      sendUpdate({ rules, filterMatches, state });
    });

    const onView = webviewPanel.onDidChangeViewState(() => {
      if (webviewPanel.active) this.activeEntry = entry;
      else if (this.activeEntry === entry) this.activeEntry = undefined;
    });

    webviewPanel.onDidDispose(() => {
      this.entries.delete(entry);
      if (this.activeEntry === entry) this.activeEntry = undefined;
      onMessage.dispose();
      onDocChange.dispose();
      onConfigChange.dispose();
      onView.dispose();
    });
  }

  /** Drive a UI command on the active panel; returns true if dispatched. */
  public sendToActive(message: HostToWebview): boolean {
    if (!this.activeEntry) return false;
    void this.activeEntry.panel.webview.postMessage(message);
    return true;
  }

  private readViewState(): ViewState {
    const cfg = vscode.workspace.getConfiguration('logViewer');
    return {
      renderAnsi: cfg.get<boolean>('renderAnsi', true),
      wordWrap: cfg.get<boolean>('wordWrap', false),
      fontSize: cfg.get<number>('fontSize', 0),
      filterMode:
        cfg.get<'highlight' | 'only-matching'>('filterMode', 'highlight'),
    };
  }

  private readRules(): FilterRule[] {
    const cfg = vscode.workspace.getConfiguration('logViewer');
    const raw = cfg.get<FilterRule[]>('filters', []);
    return raw.map((r) => ({
      name: r.name,
      pattern: r.pattern,
      regex: r.regex ?? false,
      caseSensitive: r.caseSensitive ?? false,
      color: r.color,
      enabled: r.enabled ?? true,
    }));
  }

  private post(webview: vscode.Webview, msg: HostToWebview): void {
    void webview.postMessage(msg);
  }

  private errorHtml(message: string): string {
    const nonce = getNonce();
    return /* html */ `<!DOCTYPE html>
<html><head><meta charset="UTF-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'nonce-${nonce}'">
<style nonce="${nonce}">
  body { font-family: var(--vscode-font-family); padding: 16px; color: var(--vscode-errorForeground); }
  code { font-family: var(--vscode-editor-font-family); }
</style>
</head><body>${message}</body></html>`;
  }

  private getHtmlForWebview(webview: vscode.Webview): string {
    const scriptUri = webview.asWebviewUri(
      vscode.Uri.joinPath(this.context.extensionUri, 'dist', 'webview.js'),
    );
    const nonce = getNonce();
    const csp = [
      `default-src 'none'`,
      `style-src ${webview.cspSource} 'unsafe-inline'`,
      `script-src 'nonce-${nonce}' ${webview.cspSource}`,
      `font-src ${webview.cspSource}`,
    ].join('; ');

    return /* html */ `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta http-equiv="Content-Security-Policy" content="${csp}">
<title>Log Viewer</title>
<style>
  :root { color-scheme: var(--vscode-color-scheme, dark); }
  html, body { height: 100%; margin: 0; padding: 0; }
  body {
    background: var(--vscode-editor-background);
    color: var(--vscode-editor-foreground);
    font-family: var(--vscode-editor-font-family, ui-monospace, monospace);
    font-size: var(--vscode-editor-font-size, 13px);
    display: flex;
    flex-direction: column;
  }
  .toolbar {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    padding: 6px 8px;
    border-bottom: 1px solid var(--vscode-editorWidget-border, transparent);
    background: var(--vscode-editorWidget-background, var(--vscode-editor-background));
    font-family: var(--vscode-font-family);
    font-size: 12px;
    flex: 0 0 auto;
    align-items: center;
  }
  .toolbar button, .toolbar select {
    background: var(--vscode-button-secondaryBackground, transparent);
    color: var(--vscode-button-secondaryForeground, var(--vscode-foreground));
    border: 1px solid var(--vscode-button-border, transparent);
    padding: 3px 8px;
    cursor: pointer;
    font: inherit;
    border-radius: 2px;
  }
  .toolbar button.active { background: var(--vscode-button-background); color: var(--vscode-button-foreground); }
  .toolbar button:hover { background: var(--vscode-button-secondaryHoverBackground, var(--vscode-button-hoverBackground)); }
  .toolbar input[type=search] {
    background: var(--vscode-input-background);
    color: var(--vscode-input-foreground);
    border: 1px solid var(--vscode-input-border, transparent);
    padding: 3px 6px;
    border-radius: 2px;
    font: inherit;
    min-width: 180px;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 2px 6px;
    border-radius: 10px;
    border: 1px solid var(--vscode-editorWidget-border, transparent);
    cursor: pointer;
    user-select: none;
  }
  .chip.off { opacity: 0.4; text-decoration: line-through; }
  .chip .swatch {
    width: 10px; height: 10px; border-radius: 50%;
    border: 1px solid rgba(255,255,255,0.2);
  }
  .info { margin-left: auto; opacity: 0.75; }
  .banner {
    padding: 4px 8px;
    background: var(--vscode-inputValidation-warningBackground, #6c5d00);
    color: var(--vscode-inputValidation-warningForeground, #fff);
    border-bottom: 1px solid var(--vscode-inputValidation-warningBorder, transparent);
    font-size: 12px;
  }
  .banner[hidden] { display: none; }
  #scroller {
    flex: 1 1 auto;
    overflow: auto;
    position: relative;
  }
  #spacer {
    position: relative;
    width: 100%;
  }
  #viewport {
    position: absolute;
    left: 0;
    right: 0;
    top: 0;
    will-change: transform;
  }
  .ln {
    box-sizing: border-box;
    padding: 0 12px;
    white-space: pre;
    line-height: 1.4;
    border-left: 3px solid transparent;
  }
  body.wrap .ln { white-space: pre-wrap; word-break: break-word; }
  mark.search-hit {
    background: var(--vscode-editor-findMatchHighlightBackground, rgba(255,213,0,0.5));
    color: inherit;
    border-radius: 2px;
  }
</style>
</head>
<body>
<div class="toolbar">
  <button id="btn-ansi" type="button" title="Render ANSI escape sequences as colors">ANSI</button>
  <button id="btn-wrap" type="button" title="Wrap long lines">Wrap</button>
  <button id="btn-mode" type="button" title="Toggle filter mode">Highlight</button>
  <input id="search" type="search" placeholder="Search…" />
  <button id="btn-regex" type="button" title=".* — treat search as regex">.*</button>
  <button id="btn-case" type="button" title="Aa — case sensitive search">Aa</button>
  <span id="search-info"></span>
  <span id="chips"></span>
  <button id="btn-font-down" type="button" title="Decrease font size">A−</button>
  <button id="btn-font-up" type="button" title="Increase font size">A+</button>
  <button id="btn-font-reset" type="button" title="Reset font size">A0</button>
  <button id="btn-text" type="button" title="Open in default text editor">Text Editor</button>
  <span class="info" id="info"></span>
</div>
<div class="banner" id="banner" hidden></div>
<div id="scroller">
  <div id="spacer">
    <div id="viewport"></div>
  </div>
</div>
<script nonce="${nonce}" type="module" src="${scriptUri}"></script>
</body>
</html>`;
  }
}
