import * as vscode from 'vscode';
import type { Client } from '../client.js';
import { html } from './html.js';
import { parseWebviewMessage } from './messages.js';
import { previewSettings } from './manager.js';

/**
 * `typstUltra.preview` as a custom editor.
 *
 * Registered at `priority: "option"`, so a `.typ` still opens as text unless
 * someone opts in through `workbench.editorAssociations`. We **never** write
 * that setting ourselves — mutating a user's global editor associations is the
 * anti-pattern RFC 009 called out, and claiming every `.typ` file in the world
 * because an extension was installed is exactly the behaviour it describes.
 *
 * Read-only by design: the preview shows the document, it does not edit it.
 */
export class TypstPreviewEditor implements vscode.CustomTextEditorProvider {
  static readonly viewType = 'typstUltra.preview';

  constructor(
    private readonly context: vscode.ExtensionContext,
    private readonly client: Client,
    private readonly output: vscode.OutputChannel,
  ) {}

  /** Register the provider. */
  static register(
    context: vscode.ExtensionContext,
    client: Client,
    output: vscode.OutputChannel,
  ): vscode.Disposable {
    return vscode.window.registerCustomEditorProvider(
      TypstPreviewEditor.viewType,
      new TypstPreviewEditor(context, client, output),
      {
        webviewOptions: { retainContextWhenHidden: true },
        supportsMultipleEditorsPerDocument: false,
      },
    );
  }

  async resolveCustomTextEditor(
    document: vscode.TextDocument,
    panel: vscode.WebviewPanel,
  ): Promise<void> {
    panel.webview.options = {
      enableScripts: true,
      localResourceRoots: [this.context.extensionUri],
    };
    panel.webview.html = html(panel.webview, this.context.extensionUri);

    await this.client.start(document.uri);

    const send = (message: unknown) => void panel.webview.postMessage(message);
    let seq = 0;

    const refresh = async () => {
      const metrics = await this.client.request<{
        pageCount: number;
        pages: unknown[];
      }>('typst/documentMetrics', { uri: document.uri.toString() });
      if (!metrics) return;
      send({
        type: 'metrics',
        seq: (seq += 1),
        uri: document.uri.toString(),
        pages: metrics.pages,
      });
    };

    panel.webview.onDidReceiveMessage(async (raw: unknown) => {
      const message = parseWebviewMessage(raw);
      if (!message) {
        this.output.appendLine('custom editor: dropped a malformed message');
        return;
      }

      if (message.type === 'ready') {
        send({ type: 'settings', settings: previewSettings(document.uri) });
        await refresh();
      } else if (message.type === 'viewport') {
        const pages: number[] = [];
        for (let index = message.first; index <= message.last; index += 1) {
          pages.push(index);
        }
        const mode = previewSettings(document.uri).renderMode;
        const result = await this.client.request<{ patches: unknown[] }>(
          'typst/renderPages',
          {
            uri: document.uri.toString(),
            pages,
            knownHashes: message.known,
            mode,
            ppi:
              mode === 'svg'
                ? undefined
                : Math.min(600, Math.max(72, 96 * message.zoom)),
          },
        );
        if (result) {
          send({ type: 'pages', seq: (seq += 1), patches: result.patches });
        }
      }
    });

    const subscription = this.client.onNotification('typst/compileStatus', () => {
      void refresh();
    });
    panel.onDidDispose(() => subscription.dispose());
  }
}
