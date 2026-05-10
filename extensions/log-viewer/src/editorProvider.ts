import * as vscode from 'vscode';
import { ansiToHtml, htmlEscape } from './ansi.js';
import { getNonce } from './util.js';

interface ViewState {
  renderAnsi: boolean;
  wordWrap: boolean;
}

interface RenderMessage {
  type: 'render';
  body: string;
  truncated: boolean;
  totalBytes: number;
  state: ViewState;
}

type HostToWebview = RenderMessage;

interface ReadyMessage {
  type: 'ready';
}

interface ToggleMessage {
  type: 'toggle';
  key: keyof ViewState;
}

interface OpenInTextMessage {
  type: 'openInText';
}

type WebviewToHost = ReadyMessage | ToggleMessage | OpenInTextMessage;

interface PanelEntry {
  panel: vscode.WebviewPanel;
  document: vscode.TextDocument;
  state: ViewState;
  render: () => void;
}

export class LogEditorProvider implements vscode.CustomTextEditorProvider {
  public static readonly viewType = 'logViewer.editor';

  private readonly panels = new Set<PanelEntry>();
  private activeEntry: PanelEntry | undefined;

  constructor(_context: vscode.ExtensionContext) {}

  public async resolveCustomTextEditor(
    document: vscode.TextDocument,
    webviewPanel: vscode.WebviewPanel,
    _token: vscode.CancellationToken,
  ): Promise<void> {
    const webview = webviewPanel.webview;
    webview.options = { enableScripts: true };
    webview.html = this.getHtmlForWebview(webview);

    const state: ViewState = this.readConfig();

    const render = (): void => {
      const text = document.getText();
      const totalBytes = Buffer.byteLength(text, 'utf8');
      const max = vscode.workspace
        .getConfiguration('logViewer')
        .get<number>('maxFileSizeBytes', 8 * 1024 * 1024);
      const truncated = totalBytes > max;
      const slice = truncated ? text.slice(0, Math.floor(max / 2)) : text;
      const body = state.renderAnsi ? ansiToHtml(slice) : htmlEscape(slice);
      this.post(webview, {
        type: 'render',
        body,
        truncated,
        totalBytes,
        state,
      });
    };

    const entry: PanelEntry = { panel: webviewPanel, document, state, render };
    this.panels.add(entry);
    if (webviewPanel.active) this.activeEntry = entry;

    const onMessage = webview.onDidReceiveMessage((msg: WebviewToHost) => {
      switch (msg.type) {
        case 'ready':
          render();
          break;
        case 'toggle':
          this.toggle(entry, msg.key);
          break;
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
      if (e.document.uri.toString() === document.uri.toString()) render();
    });

    const onConfigChange = vscode.workspace.onDidChangeConfiguration((e) => {
      if (!e.affectsConfiguration('logViewer')) return;
      const next = this.readConfig();
      state.renderAnsi = next.renderAnsi;
      state.wordWrap = next.wordWrap;
      render();
    });

    const onView = webviewPanel.onDidChangeViewState(() => {
      if (webviewPanel.active) this.activeEntry = entry;
      else if (this.activeEntry === entry) this.activeEntry = undefined;
    });

    webviewPanel.onDidDispose(() => {
      this.panels.delete(entry);
      if (this.activeEntry === entry) this.activeEntry = undefined;
      onMessage.dispose();
      onDocChange.dispose();
      onConfigChange.dispose();
      onView.dispose();
    });
  }

  /** Flip a state flag on the currently focused log-viewer panel, if any. */
  public toggleActive(key: keyof ViewState): boolean {
    if (!this.activeEntry) return false;
    this.toggle(this.activeEntry, key);
    return true;
  }

  private toggle(entry: PanelEntry, key: keyof ViewState): void {
    entry.state[key] = !entry.state[key];
    entry.render();
  }

  private readConfig(): ViewState {
    const cfg = vscode.workspace.getConfiguration('logViewer');
    return {
      renderAnsi: cfg.get<boolean>('renderAnsi', true),
      wordWrap: cfg.get<boolean>('wordWrap', false),
    };
  }

  private post(webview: vscode.Webview, msg: HostToWebview): void {
    void webview.postMessage(msg);
  }

  private getHtmlForWebview(webview: vscode.Webview): string {
    const nonce = getNonce();
    const csp = [
      `default-src 'none'`,
      `style-src ${webview.cspSource} 'unsafe-inline'`,
      `script-src 'nonce-${nonce}'`,
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
    gap: 6px;
    padding: 6px 8px;
    border-bottom: 1px solid var(--vscode-editorWidget-border, transparent);
    background: var(--vscode-editorWidget-background, var(--vscode-editor-background));
    font-family: var(--vscode-font-family);
    font-size: 12px;
    flex: 0 0 auto;
    align-items: center;
  }
  .toolbar button {
    background: var(--vscode-button-secondaryBackground, transparent);
    color: var(--vscode-button-secondaryForeground, var(--vscode-foreground));
    border: 1px solid var(--vscode-button-border, transparent);
    padding: 3px 8px;
    cursor: pointer;
    font: inherit;
    border-radius: 2px;
  }
  .toolbar button.active {
    background: var(--vscode-button-background);
    color: var(--vscode-button-foreground);
  }
  .toolbar button:hover {
    background: var(--vscode-button-secondaryHoverBackground, var(--vscode-button-hoverBackground));
  }
  .toolbar .info { margin-left: auto; opacity: 0.75; }
  .banner {
    padding: 4px 8px;
    background: var(--vscode-inputValidation-warningBackground, #6c5d00);
    color: var(--vscode-inputValidation-warningForeground, #fff);
    border-bottom: 1px solid var(--vscode-inputValidation-warningBorder, transparent);
    font-size: 12px;
  }
  .banner[hidden] { display: none; }
  #log {
    flex: 1 1 auto;
    overflow: auto;
    margin: 0;
    padding: 8px 12px;
    white-space: pre;
    tab-size: 4;
    line-height: 1.4;
  }
  body.wrap #log { white-space: pre-wrap; word-break: break-word; }
</style>
</head>
<body>
<div class="toolbar">
  <button id="btn-ansi" type="button" title="Render ANSI escape sequences as colors">ANSI</button>
  <button id="btn-wrap" type="button" title="Wrap long lines">Wrap</button>
  <button id="btn-text" type="button" title="Open this file in the default text editor">Open in Text Editor</button>
  <span class="info" id="info"></span>
</div>
<div class="banner" id="banner" hidden></div>
<pre id="log"></pre>
<script nonce="${nonce}">
  const vscode = acquireVsCodeApi();
  const logEl = document.getElementById('log');
  const banner = document.getElementById('banner');
  const info = document.getElementById('info');
  const btnAnsi = document.getElementById('btn-ansi');
  const btnWrap = document.getElementById('btn-wrap');
  const btnText = document.getElementById('btn-text');

  let state = { renderAnsi: true, wordWrap: false };

  function applyState() {
    btnAnsi.classList.toggle('active', state.renderAnsi);
    btnWrap.classList.toggle('active', state.wordWrap);
    document.body.classList.toggle('wrap', state.wordWrap);
  }

  function fmtBytes(n) {
    if (n < 1024) return n + ' B';
    if (n < 1024 * 1024) return (n / 1024).toFixed(1) + ' KB';
    return (n / (1024 * 1024)).toFixed(1) + ' MB';
  }

  btnAnsi.addEventListener('click', () => vscode.postMessage({ type: 'toggle', key: 'renderAnsi' }));
  btnWrap.addEventListener('click', () => vscode.postMessage({ type: 'toggle', key: 'wordWrap' }));
  btnText.addEventListener('click', () => vscode.postMessage({ type: 'openInText' }));

  window.addEventListener('message', (e) => {
    const msg = e.data;
    if (msg.type === 'render') {
      state = msg.state;
      applyState();
      logEl.innerHTML = msg.body;
      info.textContent = fmtBytes(msg.totalBytes);
      if (msg.truncated) {
        banner.hidden = false;
        banner.textContent = 'File exceeds logViewer.maxFileSizeBytes; only the head is shown. Open in Text Editor to see the full file.';
      } else {
        banner.hidden = true;
      }
    }
  });

  vscode.postMessage({ type: 'ready' });
</script>
</body>
</html>`;
  }
}
