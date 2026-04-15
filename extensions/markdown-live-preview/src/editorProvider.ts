import * as vscode from 'vscode';
import type { EditorMode, HostToWebviewMessage, WebviewToHostMessage } from './messages';
import { getNonce } from './util';

export class LivePreviewEditorProvider implements vscode.CustomTextEditorProvider {
  public static readonly viewType = 'markdownLivePreview.editor';

  constructor(private readonly context: vscode.ExtensionContext) {}

  public async resolveCustomTextEditor(
    document: vscode.TextDocument,
    webviewPanel: vscode.WebviewPanel,
    _token: vscode.CancellationToken,
  ): Promise<void> {
    const webview = webviewPanel.webview;
    webview.options = {
      enableScripts: true,
      localResourceRoots: [
        vscode.Uri.joinPath(this.context.extensionUri, 'dist', 'webview'),
      ],
    };

    webview.html = this.getHtmlForWebview(webview);

    const defaultMode = vscode.workspace
      .getConfiguration('markdownLivePreview')
      .get<EditorMode>('defaultMode', 'live-preview');

    // Send initial document content once webview is ready
    const initDisposable = webview.onDidReceiveMessage((msg) => {
      if (msg.type === 'webview:ready') {
        initDisposable.dispose();
        this.postMessage(webview, {
          type: 'doc:init',
          content: document.getText(),
          uri: document.uri.toString(),
          mode: defaultMode,
        });
      }
    });

    // Handle messages from the webview
    const messageDisposable = webview.onDidReceiveMessage((msg: WebviewToHostMessage) => {
      switch (msg.type) {
        case 'edit:apply':
          this.applyEdit(document, webview, msg);
          break;
        case 'cursor:changed':
          // Could be used for status bar updates in the future
          break;
        case 'mode:changed':
          // Track mode state if needed
          break;
      }
    });

    // Forward document changes from external sources to the webview
    const docChangeDisposable = vscode.workspace.onDidChangeTextDocument((e) => {
      if (e.document.uri.toString() !== document.uri.toString()) return;
      if (e.contentChanges.length === 0) return;

      this.postMessage(webview, {
        type: 'doc:update',
        changes: e.contentChanges.map((c) => ({
          rangeOffset: c.rangeOffset,
          rangeLength: c.rangeLength,
          text: c.text,
        })),
        version: e.document.version,
      });
    });

    // Forward config changes
    const configDisposable = vscode.workspace.onDidChangeConfiguration((e) => {
      if (
        e.affectsConfiguration('editor') ||
        e.affectsConfiguration('markdownLivePreview')
      ) {
        this.sendConfigUpdate(webview);
      }
    });

    webviewPanel.onDidDispose(() => {
      initDisposable.dispose();
      messageDisposable.dispose();
      docChangeDisposable.dispose();
      configDisposable.dispose();
    });
  }

  private async applyEdit(
    document: vscode.TextDocument,
    webview: vscode.Webview,
    msg: { startLine: number; endLine: number; newText: string; version: number },
  ): Promise<void> {
    const edit = new vscode.WorkspaceEdit();
    const startPos = new vscode.Position(msg.startLine, 0);
    const endPos =
      msg.endLine >= document.lineCount
        ? document.lineAt(document.lineCount - 1).range.end
        : new vscode.Position(msg.endLine, 0);

    edit.replace(document.uri, new vscode.Range(startPos, endPos), msg.newText);
    const success = await vscode.workspace.applyEdit(edit);

    if (success) {
      this.postMessage(webview, {
        type: 'edit:ack',
        version: document.version,
      });
    }
  }

  private sendConfigUpdate(webview: vscode.Webview): void {
    const editorConfig = vscode.workspace.getConfiguration('editor');
    const themeKind = vscode.window.activeColorTheme.kind;
    let theme: 'light' | 'dark' | 'high-contrast';
    switch (themeKind) {
      case vscode.ColorThemeKind.Light:
        theme = 'light';
        break;
      case vscode.ColorThemeKind.HighContrast:
      case vscode.ColorThemeKind.HighContrastLight:
        theme = 'high-contrast';
        break;
      default:
        theme = 'dark';
    }

    this.postMessage(webview, {
      type: 'config:update',
      fontSize: editorConfig.get<number>('fontSize', 14),
      fontFamily: editorConfig.get<string>('fontFamily', 'monospace'),
      theme,
    });
  }

  private postMessage(webview: vscode.Webview, message: HostToWebviewMessage): void {
    webview.postMessage(message);
  }

  private getHtmlForWebview(webview: vscode.Webview): string {
    const scriptUri = webview.asWebviewUri(
      vscode.Uri.joinPath(this.context.extensionUri, 'dist', 'webview', 'index.js'),
    );
    const styleUri = webview.asWebviewUri(
      vscode.Uri.joinPath(this.context.extensionUri, 'dist', 'webview', 'style.css'),
    );

    const nonce = getNonce();

    return /* html */ `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <meta http-equiv="Content-Security-Policy"
    content="default-src 'none';
      style-src ${webview.cspSource} 'unsafe-inline';
      script-src 'nonce-${nonce}';
      font-src ${webview.cspSource};
      img-src ${webview.cspSource} data: https:;">
  <title>Markdown Live Preview</title>
  <link rel="stylesheet" href="${styleUri}">
</head>
<body>
  <div id="root"></div>
  <script nonce="${nonce}" type="module" src="${scriptUri}"></script>
</body>
</html>`;
  }
}
