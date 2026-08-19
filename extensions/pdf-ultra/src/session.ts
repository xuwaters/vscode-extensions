import * as vscode from 'vscode';
import { CONFIG_SECTION, readReloadOnChange, readRememberPosition, readSettings } from './config.js';
import type { PdfDocument } from './document.js';
import { html } from './html.js';
import {
  isAllowedLink,
  parseWebviewMessage,
  type DocumentSource,
  type HostToWebview,
  type PdfAssetUrls,
  type ViewerCommand,
  type ViewerPlace,
  type WebviewToHost,
} from './messages.js';
import type { PlaceMemory } from './placeMemory.js';

/**
 * Bytes per chunk of the `bytes` fallback.
 *
 * Divisible by three so each chunk encodes to base64 without padding, which is
 * what lets the page concatenate the encoded chunks and decode once.
 */
export const CHUNK_BYTES = 524_286;

/**
 * One open tab.
 *
 * A tab is bound to its document for its life — VSCode owns these webviews and
 * gives each one file — so there is no retargeting here, no following the
 * active editor, and no pinning. What the session does own is everything that
 * outlives a single message: the document's bytes on the way in, the reader's
 * place on the way out, and the reload when the file is rebuilt underneath them.
 */
export class ViewerSession {
  private readonly disposables: vscode.Disposable[] = [];
  private latest: ViewerPlace | undefined;
  private pageCount = 0;
  private disposed = false;

  /** Fires whenever this tab's page or page count changes. */
  readonly onDidChangePlace = new vscode.EventEmitter<ViewerSession>();

  constructor(
    private readonly document: PdfDocument,
    private readonly panel: vscode.WebviewPanel,
    private readonly extensionUri: vscode.Uri,
    private readonly output: vscode.OutputChannel,
    private readonly places: PlaceMemory,
  ) {
    panel.webview.options = {
      enableScripts: true,
      localResourceRoots: [extensionUri, vscode.Uri.joinPath(document.uri, '..')],
    };
    panel.webview.html = html(panel.webview, extensionUri);

    this.disposables.push(
      panel.webview.onDidReceiveMessage((raw: unknown) => {
        const message = parseWebviewMessage(raw);
        if (!message) {
          this.output.appendLine('viewer: dropped a malformed message');
          return;
        }
        void this.onMessage(message);
      }),
      document.onDidChange(() => {
        if (!readReloadOnChange(this.uri)) return;
        void this.reload();
      }),
      document.onDidDelete(() => {
        this.send({
          type: 'hostError',
          message: 'This file no longer exists on disk.',
        });
      }),
      vscode.workspace.onDidChangeConfiguration((event) => {
        if (!event.affectsConfiguration(CONFIG_SECTION, this.uri)) return;
        this.send({ type: 'settings', settings: readSettings(this.uri) });
      }),
      // A retained webview is kept alive in the background, and a background
      // webview is given no animation frames — which is what pdf.js continues a
      // page render on. So a tab returning to the front may be carrying pages
      // that stopped drawing half way, and this is the only event that says so:
      // nothing about the document, the settings or the scroller has changed.
      panel.onDidChangeViewState(() => {
        if (panel.visible) this.send({ type: 'visible' });
        // Coming back to this tab from the keyboard focuses the page but nothing
        // in it, and the keys that turn the pages act on whatever holds the
        // focus. Without this the reader has to click the page before → turns it.
        if (panel.active) this.send({ type: 'focus' });
      }),
    );
  }

  get uri(): vscode.Uri {
    return this.document.uri;
  }

  /** Whether this tab is the one the reader is looking at. */
  get active(): boolean {
    return this.panel.active;
  }

  /** How many pages the open document has, or 0 before it opens. */
  get pages(): number {
    return this.pageCount;
  }

  /** The page being read, 1-based, or undefined before the first report. */
  get page(): number | undefined {
    return this.latest?.page;
  }

  /** Ask the page to do something — from the title bar, a key, or the palette. */
  command(command: ViewerCommand, page?: number): void {
    this.send({ type: 'command', command, page });
  }

  /** Bring this tab forward. */
  reveal(): void {
    this.panel.reveal(this.panel.viewColumn, false);
  }

  /** Re-read the file, keeping the reader's place — the Reload command. */
  async reloadNow(): Promise<void> {
    await this.reload();
  }

  private async onMessage(message: WebviewToHost): Promise<void> {
    switch (message.type) {
      case 'ready': {
        const restore = readRememberPosition(this.uri)
          ? this.places.get(this.uri)
          : undefined;
        this.send({
          type: 'open',
          name: this.document.name,
          source: await this.source(),
          assets: this.assets(),
          settings: readSettings(this.uri),
          restore,
        });
        break;
      }

      case 'opened':
        this.pageCount = message.pageCount;
        this.onDidChangePlace.fire(this);
        break;

      case 'place':
        this.latest = message.place;
        this.onDidChangePlace.fire(this);
        break;

      case 'needBytes':
        // The resource URL did not load — a virtual file system, or a resource
        // server that declined it. Ship the file over the message channel
        // instead of leaving the reader with a blank tab.
        this.output.appendLine(`viewer: falling back to byte transfer (${message.reason})`);
        await this.sendBytes();
        break;

      case 'openLink':
        if (isAllowedLink(message.href)) {
          void vscode.env.openExternal(vscode.Uri.parse(message.href));
        } else {
          this.output.appendLine(`viewer: refused to open ${message.href}`);
        }
        break;

      case 'pagePng':
        await this.savePng(message.page, message.data);
        break;

      case 'failed':
        this.output.appendLine(`viewer: ${this.document.name}: ${message.message}`);
        break;

      case 'error':
        this.output.appendLine(`viewer: ${message.context}: ${message.message}`);
        break;
    }
  }

  /**
   * Where the page should read the document from.
   *
   * The URL path is the one that matters: VSCode's resource server streams the
   * file straight into the webview, so a 200 MB document never passes through
   * the extension host and never becomes a string. Anything that has no such
   * URL — a document from a virtual file-system provider — is announced as
   * `bytes` and follows over the message channel.
   */
  private async source(): Promise<DocumentSource> {
    const url = this.panel.webview.asWebviewUri(this.uri);
    if (url.scheme === 'http' || url.scheme === 'https') {
      // The revision is a cache-buster: without it a reload of a rewritten file
      // can be served the bytes the viewer already has.
      return {
        kind: 'url',
        url: url.with({ query: `pdfUltraRev=${this.document.version}` }).toString(),
      };
    }
    let byteLength = 0;
    try {
      byteLength = (await vscode.workspace.fs.stat(this.uri)).size;
    } catch {
      // Left at zero: the page shows a progress bar it cannot fill rather than
      // refusing to try.
    }
    return { kind: 'bytes', byteLength };
  }

  /**
   * pdf.js's out-of-bundle data, as URLs the page may load. The trailing
   * slashes are load-bearing: pdf.js appends a filename to each.
   */
  private assets(): PdfAssetUrls {
    const at = (...parts: string[]): string =>
      this.panel.webview
        .asWebviewUri(vscode.Uri.joinPath(this.extensionUri, 'dist', ...parts))
        .toString();
    return {
      worker: at('pdf.worker.js'),
      cMap: `${at('pdfjs', 'cmaps')}/`,
      standardFont: `${at('pdfjs', 'standard_fonts')}/`,
      wasm: `${at('pdfjs', 'wasm')}/`,
    };
  }

  private async reload(): Promise<void> {
    this.send({ type: 'reload', source: await this.source() });
  }

  private async sendBytes(): Promise<void> {
    let bytes: Uint8Array;
    try {
      bytes = await vscode.workspace.fs.readFile(this.uri);
    } catch (error) {
      this.send({
        type: 'hostError',
        message: `Could not read the file: ${describe(error)}`,
      });
      return;
    }
    const total = Math.max(1, Math.ceil(bytes.byteLength / CHUNK_BYTES));
    for (let index = 0; index < total; index += 1) {
      if (this.disposed) return;
      const slice = bytes.subarray(index * CHUNK_BYTES, (index + 1) * CHUNK_BYTES);
      this.send({
        type: 'chunk',
        index,
        total,
        data: Buffer.from(slice).toString('base64'),
      });
    }
  }

  private async savePng(page: number, data: string): Promise<void> {
    const stem = this.document.name.replace(/\.pdf$/i, '');
    const target = await vscode.window.showSaveDialog({
      defaultUri: vscode.Uri.joinPath(this.uri, '..', `${stem}-p${page}.png`),
      filters: { Images: ['png'] },
      title: 'Export page as PNG',
    });
    if (!target) return;
    try {
      await vscode.workspace.fs.writeFile(target, Buffer.from(data, 'base64'));
      void vscode.window.showInformationMessage(`Wrote page ${page} to ${target.fsPath}`);
    } catch (error) {
      void vscode.window.showErrorMessage(`Could not write the PNG: ${describe(error)}`);
    }
  }

  private send(message: HostToWebview): void {
    if (this.disposed) return;
    void this.panel.webview.postMessage(message);
  }

  /**
   * Park the reader's place on the way out. Written on dispose rather than on
   * every scroll: the position only matters to a *later* open, and a write to
   * workspace state per scroll frame is a lot of writes for one number.
   */
  async close(): Promise<void> {
    this.disposed = true;
    for (const disposable of this.disposables) disposable.dispose();
    this.disposables.length = 0;
    const place = this.latest;
    this.onDidChangePlace.dispose();
    if (place && readRememberPosition(this.uri)) {
      await this.places.park(this.uri, place);
    }
  }
}

function describe(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
