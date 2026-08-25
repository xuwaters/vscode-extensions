// Read-only JSON Lines table preview.
//
// `CustomTextEditorProvider` rather than a readonly CustomDocument
// because a .jsonl file *is* text: VSCode keeps owning the document's
// life (encoding, watchers, the text editor the user can reopen at any
// time), and this provider simply never produces an edit. Registered at
// `priority: "option"` — the text editor stays the default; the table
// is one "Reopen Editor With…" (or the title-bar button) away.

import * as path from 'path';
import * as vscode from 'vscode';
import type { AnalyzerBridge } from '../analyzer.js';
import { readPreviewMaxFileSize, readPreviewMaxRows } from '../config.js';
import { parseWebviewMessage, type HostToWebview } from '../messages.js';
import { previewHtml } from './html.js';

const RELOAD_DEBOUNCE_MS = 300;

export class JsonlPreviewProvider implements vscode.CustomTextEditorProvider {
  static readonly viewType = 'jsonUltra.jsonlPreview';

  constructor(
    private readonly context: vscode.ExtensionContext,
    private readonly bridge: AnalyzerBridge,
    private readonly log: (message: string) => void,
  ) {}

  static register(
    context: vscode.ExtensionContext,
    bridge: AnalyzerBridge,
    log: (message: string) => void,
  ): vscode.Disposable {
    return vscode.window.registerCustomEditorProvider(
      JsonlPreviewProvider.viewType,
      new JsonlPreviewProvider(context, bridge, log),
      {
        webviewOptions: { retainContextWhenHidden: true },
        supportsMultipleEditorsPerDocument: true,
      },
    );
  }

  resolveCustomTextEditor(
    document: vscode.TextDocument,
    panel: vscode.WebviewPanel,
  ): void {
    panel.webview.options = {
      enableScripts: true,
      localResourceRoots: [this.context.extensionUri],
    };
    panel.webview.html = previewHtml(panel.webview, this.context.extensionUri);

    const send = (message: HostToWebview) => {
      void panel.webview.postMessage(message);
    };

    let reloadTimer: ReturnType<typeof setTimeout> | undefined;
    const scheduleLoad = () => {
      if (reloadTimer !== undefined) clearTimeout(reloadTimer);
      reloadTimer = setTimeout(() => {
        reloadTimer = undefined;
        this.load(document, send);
      }, RELOAD_DEBOUNCE_MS);
    };

    const disposables: vscode.Disposable[] = [
      panel.webview.onDidReceiveMessage((raw: unknown) => {
        const message = parseWebviewMessage(raw);
        if (!message) {
          this.log('preview: dropped a malformed message');
          return;
        }
        void this.onMessage(message, document, send);
      }),
      vscode.workspace.onDidChangeTextDocument((event) => {
        if (event.document !== document) return;
        if (event.contentChanges.length === 0) return;
        scheduleLoad();
      }),
      vscode.workspace.onDidChangeConfiguration((event) => {
        if (event.affectsConfiguration('jsonUltra.preview', document.uri)) scheduleLoad();
      }),
      panel.onDidChangeViewState(() => {
        // A hidden webview gets no animation frames; nudge a repaint.
        if (panel.visible) send({ type: 'visible' });
      }),
    ];
    panel.onDidDispose(() => {
      if (reloadTimer !== undefined) clearTimeout(reloadTimer);
      for (const disposable of disposables) disposable.dispose();
    });
  }

  private async onMessage(
    message: NonNullable<ReturnType<typeof parseWebviewMessage>>,
    document: vscode.TextDocument,
    send: (message: HostToWebview) => void,
  ): Promise<void> {
    switch (message.type) {
      case 'ready':
        this.load(document, send);
        break;
      case 'openLine': {
        const line = Math.min(message.line, Math.max(0, document.lineCount - 1));
        await vscode.window.showTextDocument(document, {
          viewColumn: vscode.ViewColumn.Beside,
          preserveFocus: false,
          selection: document.lineAt(line).range,
        });
        break;
      }
      case 'openText':
        await vscode.commands.executeCommand('vscode.openWith', document.uri, 'default');
        break;
      case 'copy':
        await vscode.env.clipboard.writeText(message.text);
        break;
      case 'error':
        this.log(`preview page error: ${message.message}`);
        break;
    }
  }

  private load(document: vscode.TextDocument, send: (message: HostToWebview) => void): void {
    if (!this.bridge.available) {
      send({ type: 'noParser' });
      return;
    }
    const text = document.getText();
    const limit = readPreviewMaxFileSize(document.uri);
    if (text.length > limit) {
      send({ type: 'refused', bytes: text.length, limit });
      return;
    }
    const uri = document.uri.toString();
    // The preview always reads the file as JSON Lines, whatever language
    // id the tab happens to carry.
    this.bridge.updateFile(uri, text, 'jsonl');
    const table = this.bridge.jsonlTable(uri, readPreviewMaxRows(document.uri));
    if (!table) {
      send({ type: 'noParser' });
      return;
    }
    send({ type: 'load', name: path.basename(document.uri.fsPath), table });
  }
}
