import * as vscode from 'vscode';
import type { Client } from '../lsp/client.js';
import type { RootState } from '../compileRoot.js';
import { PREVIEW_EDITOR_VIEW_TYPE, openSource } from './editors.js';
import { html } from './html.js';
import { previewSettings } from './manager.js';
import {
  isAllowedLink,
  parseWebviewMessage,
  type HostToWebview,
  type WebviewToHost,
} from './messages.js';
import type { PageMemory } from './pageMemory.js';
import { readPlace, writePlace } from './place.js';
import { compileNow, fetchMetrics, fetchPages, jumpFromClick } from './rpc.js';

/**
 * The full-tab preview — what Preview mode swaps the source editor for.
 *
 * Registered at `priority: "option"`, so a `.typ` still opens as text unless
 * someone opts in through `workbench.editorAssociations`. We **never** write
 * that setting ourselves — mutating a user's global editor associations is the
 * anti-pattern RFC 009 called out, and claiming every `.typ` file in the world
 * because an extension was installed is exactly the behaviour it describes.
 * Preview mode reaches this surface by asking for it by name instead.
 *
 * Read-only by design: the preview shows the document, it does not edit it.
 * VSCode owns these webviews, one per tab, and binds each to its document for
 * the tab's life — so unlike the following panel there is nothing to retarget.
 * The way back to the text editor is a mode switch, which hands this tab over
 * to it.
 */
export class TypstPreviewEditor implements vscode.CustomTextEditorProvider {
  static readonly viewType = PREVIEW_EDITOR_VIEW_TYPE;

  constructor(
    private readonly context: vscode.ExtensionContext,
    private readonly client: Client,
    private readonly output: vscode.OutputChannel,
    private readonly pages: PageMemory,
    private readonly root: RootState,
  ) {}

  /** Register the provider. */
  static register(
    context: vscode.ExtensionContext,
    client: Client,
    output: vscode.OutputChannel,
    pages: PageMemory,
    root: RootState,
  ): vscode.Disposable {
    return vscode.window.registerCustomEditorProvider(
      TypstPreviewEditor.viewType,
      new TypstPreviewEditor(context, client, output, pages, root),
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

    const uri = document.uri;
    const key = uri.toString();
    const send = (message: HostToWebview) => void panel.webview.postMessage(message);
    let seq = 0;

    const refresh = async (): Promise<void> => {
      const metrics = await fetchMetrics(this.client, uri);
      if (!metrics) return;
      send({ type: 'metrics', seq: (seq += 1), uri: key, pages: metrics.pages });
    };

    /**
     * Make this tab's document the one the server compiles.
     *
     * A full-tab preview is not a text editor, so activating it fires no event
     * the server hears. Without this, switching between two preview tabs would
     * leave both showing whichever document was typed in last.
     */
    const claimCompile = (): void => {
      compileNow(this.client, uri, this.root.entry !== undefined);
    };

    panel.webview.onDidReceiveMessage(async (raw: unknown) => {
      const message = parseWebviewMessage(raw);
      if (!message) {
        this.output.appendLine('custom editor: dropped a malformed message');
        return;
      }
      await this.onMessage(message, document, panel, {
        refresh,
        claimCompile,
        send,
        seq: () => (seq += 1),
      });
    });

    const subscription = this.client.onNotification('typst/compileStatus', () => {
      void refresh();
    });

    const viewState = panel.onDidChangeViewState(() => {
      if (panel.active) claimCompile();
    });

    panel.onDidDispose(() => {
      subscription.dispose();
      viewState.dispose();
    });
  }

  private async onMessage(
    message: WebviewToHost,
    document: vscode.TextDocument,
    panel: vscode.WebviewPanel,
    tab: {
      refresh: () => Promise<void>;
      claimCompile: () => void;
      send: (message: HostToWebview) => void;
      seq: () => number;
    },
  ): Promise<void> {
    const uri = document.uri;

    switch (message.type) {
      case 'ready': {
        // A mode switch opens a *new* webview, so the fit and zoom the reader
        // had in the panel they came from arrive with the settings — otherwise
        // switching to Preview view would silently undo them.
        tab.send({
          type: 'init',
          settings: previewSettings(uri),
          restore: readPlace(this.context),
        });
        tab.claimCompile();
        await tab.refresh();
        // Arriving from the text editor by way of a mode switch: pick the
        // reader up on the page they were on, rather than at page one.
        const page = this.pages.peek(uri.toString());
        if (page !== undefined && page > 0) tab.send({ type: 'goToPage', page });
        break;
      }

      case 'viewport': {
        const result = await fetchPages(
          this.client,
          uri,
          message.first,
          message.last,
          message.known,
          message.zoom,
        );
        if (result) {
          tab.send({ type: 'pages', seq: tab.seq(), patches: result.patches });
        }
        break;
      }

      case 'click': {
        const result = await jumpFromClick(
          this.client,
          message.page,
          message.xPt,
          message.yPt,
        );
        if (!result) break;
        if (result.kind === 'url') {
          if (isAllowedLink(result.url)) {
            void vscode.env.openExternal(vscode.Uri.parse(result.url));
          }
          break;
        }
        if (result.kind === 'page') {
          tab.send({ type: 'goToPage', page: result.page });
          break;
        }
        // Clicking a page means "show me that bit of the source" — not "give me
        // a source to show it in". With the file already split out beside us
        // there is an editor to put on the line; with none, the reader is
        // looking at a preview, and rearranging their layout under a click is
        // the wrong kind of helpful.
        const editor = vscode.window.visibleTextEditors.find(
          (candidate) => candidate.document.uri.toString() === result.uri,
        );
        if (!editor) break;
        const position = new vscode.Position(
          result.position.line,
          result.position.character,
        );
        editor.selection = new vscode.Selection(position, position);
        editor.revealRange(
          new vscode.Range(position, position),
          vscode.TextEditorRevealType.InCenterIfOutsideViewport,
        );
        break;
      }

      case 'scrolled':
        // Where a switch back to Edit or Split hands the reader over.
        this.pages.park(uri.toString(), message.page);
        break;

      case 'openLink':
        if (isAllowedLink(message.href)) {
          void vscode.env.openExternal(vscode.Uri.parse(message.href));
        } else {
          this.output.appendLine(`preview: refused to open ${message.href}`);
        }
        break;

      case 'export':
        await vscode.commands.executeCommand('typstUltra.export', uri);
        break;

      case 'openSource':
        // The toolbar's Edit button: the source takes this tab over, keeping
        // its place in the tab bar and its unsaved changes.
        await openSource(uri, panel.viewColumn ?? vscode.ViewColumn.One);
        break;

      case 'state':
        await writePlace(this.context, {
          zoom: message.zoom,
          fit: message.fit,
          inverted: message.inverted,
        });
        break;

      case 'error':
        this.output.appendLine(`preview: ${message.context}: ${message.message}`);
        break;
    }
  }
}
