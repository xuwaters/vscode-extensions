import * as vscode from 'vscode';
import type { Client } from '../client.js';
import * as config from '../config.js';
import {
  isAllowedLink,
  parseWebviewMessage,
  type HostToWebview,
  type PageMetric,
  type PagePatch,
  type PreviewSettings,
  type WebviewToHost,
} from './messages.js';
import { html } from './html.js';
import { SyncGuard } from './sync.js';

/** The server's answer to `typst/documentMetrics`. */
interface MetricsResult {
  pageCount: number;
  pages: PageMetric[];
}

/** The server's answer to `typst/renderPages`. */
interface RenderResult {
  patches: PagePatch[];
  pageCount: number;
}

/** The server's answer to `typst/jumpFromClick`. */
type JumpResult =
  | { kind: 'source'; uri: string; position: { line: number; character: number } }
  | { kind: 'url'; url: string }
  | { kind: 'page'; page: number; xPt: number; yPt: number };

/**
 * The preview panel.
 *
 * One panel that follows the active `.typ` editor, matching
 * markdown-preview-ultra's model. Multiple simultaneous previews are
 * deliberately not supported: two panels means two answers to "which document
 * is this", and every sync rule then needs a caveat.
 */
export class PreviewManager implements vscode.Disposable {
  private panel: vscode.WebviewPanel | undefined;
  private target: vscode.Uri | undefined;
  private locked = false;
  private seq = 0;
  private readonly disposables: vscode.Disposable[] = [];
  private readonly guard = new SyncGuard();

  constructor(
    private readonly context: vscode.ExtensionContext,
    private readonly client: Client,
    private readonly output: vscode.OutputChannel,
  ) {
    this.disposables.push(
      vscode.window.onDidChangeActiveTextEditor((editor) => {
        if (!this.locked && editor?.document.languageId === 'typst') {
          this.retarget(editor.document.uri);
        }
      }),
      vscode.window.onDidChangeTextEditorSelection((event) =>
        this.onCursorMoved(event),
      ),
      vscode.workspace.onDidChangeConfiguration((event) => {
        if (event.affectsConfiguration('typstUltra.preview')) this.pushSettings();
      }),
    );

    // The server tells us when a compile finished; that is the moment to
    // re-measure and refresh whatever the viewport is showing.
    this.disposables.push(
      this.client.onNotification('typst/compileStatus', (params) => {
        this.onCompileStatus(params);
      }),
    );
  }

  /** Whether a preview panel exists and is visible. */
  get visible(): boolean {
    return this.panel?.visible ?? false;
  }

  /** Open, or reveal, the preview for a document. */
  async show(uri: vscode.Uri, column: vscode.ViewColumn): Promise<void> {
    this.target = uri;

    if (this.panel) {
      this.panel.reveal(column, true);
      await this.refreshMetrics();
      return;
    }

    const panel = vscode.window.createWebviewPanel(
      'typstUltra.preview',
      `Preview ${uriLabel(uri)}`,
      { viewColumn: column, preserveFocus: true },
      {
        enableScripts: true,
        retainContextWhenHidden: true,
        // Free, and it works because the SVG carries real `<text>` runs.
        enableFindWidget: true,
        localResourceRoots: [this.context.extensionUri],
      },
    );

    this.adopt(panel);
    await this.refreshMetrics();
  }

  /** Attach to a panel, whether newly created or restored by the serializer. */
  adopt(panel: vscode.WebviewPanel): void {
    this.panel = panel;
    panel.webview.html = html(panel.webview, this.context.extensionUri);

    panel.webview.onDidReceiveMessage((raw: unknown) => {
      const message = parseWebviewMessage(raw);
      if (!message) {
        this.output.appendLine(
          `preview: dropped a message that did not match any known shape`,
        );
        return;
      }
      void this.onMessage(message);
    });

    panel.onDidDispose(() => {
      this.panel = undefined;
      void vscode.commands.executeCommand(
        'setContext',
        'typstUltra.previewVisible',
        false,
      );
    });

    void vscode.commands.executeCommand(
      'setContext',
      'typstUltra.previewVisible',
      true,
    );
  }

  /** Point the preview at a different document. */
  retarget(uri: vscode.Uri): void {
    if (this.target?.toString() === uri.toString()) return;
    this.target = uri;
    if (this.panel) this.panel.title = `Preview ${uriLabel(uri)}`;
    void this.refreshMetrics();
  }

  /** Stop following the active editor. */
  toggleLock(): boolean {
    this.locked = !this.locked;
    if (this.panel) {
      this.panel.title = `${this.locked ? '$(lock) ' : ''}Preview ${uriLabel(this.target)}`;
    }
    return this.locked;
  }

  /** Scroll the preview to wherever the cursor is. */
  async syncToCursor(): Promise<void> {
    const editor = vscode.window.activeTextEditor;
    if (!editor || !this.panel || !this.target) return;
    await this.sendCursor(editor, true);
  }

  /** Flip colour inversion without writing the setting. */
  toggleInvert(): void {
    this.post({ type: 'goToPage', page: -1 });
  }

  dispose(): void {
    for (const disposable of this.disposables) disposable.dispose();
    this.panel?.dispose();
  }

  private async onMessage(message: WebviewToHost): Promise<void> {
    switch (message.type) {
      case 'ready':
        this.pushSettings();
        await this.refreshMetrics();
        break;

      case 'viewport':
        await this.renderPages(
          message.first,
          message.last,
          message.known,
          message.zoom,
        );
        break;

      case 'click':
        await this.jumpFromClick(message.page, message.xPt, message.yPt);
        break;

      case 'scrolled':
        await this.onPreviewScrolled(message.page, message.yPt);
        break;

      case 'openLink':
        if (isAllowedLink(message.href)) {
          void vscode.env.openExternal(vscode.Uri.parse(message.href));
        } else {
          this.output.appendLine(`preview: refused to open ${message.href}`);
        }
        break;

      case 'state':
        await this.context.workspaceState.update('typstUltra.previewState', {
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

  private onCompileStatus(params: unknown): void {
    if (typeof params !== 'object' || params === null) return;
    const state = (params as { state?: string }).state;
    if (state !== 'compiling' && state !== 'ok' && state !== 'error') return;

    this.post({ type: 'status', state });
    if (state === 'ok') void this.refreshMetrics();
  }

  /** Ask the server how many pages there are and how big they are. */
  private async refreshMetrics(): Promise<void> {
    if (!this.panel || !this.target) return;

    const result = await this.client.request<MetricsResult>(
      'typst/documentMetrics',
      { uri: this.target.toString() },
    );
    if (!result) return;

    this.post({
      type: 'metrics',
      seq: ++this.seq,
      uri: this.target.toString(),
      pages: result.pages,
    });
  }

  /** Fetch the pages in view that the webview does not already hold. */
  private async renderPages(
    first: number,
    last: number,
    known: Record<number, string>,
    zoom: number,
  ): Promise<void> {
    if (!this.target) return;

    const pages: number[] = [];
    for (let index = first; index <= last; index += 1) pages.push(index);

    const mode = config.read(this.target).host.preview.renderMode;
    const result = await this.client.request<RenderResult>('typst/renderPages', {
      uri: this.target.toString(),
      pages,
      knownHashes: known,
      mode,
      // A raster page is baked at one resolution, so it has to be rendered for
      // the zoom it will be shown at. 96 dpi is 1:1 with a CSS pixel.
      ppi: mode === 'svg' ? undefined : Math.min(600, Math.max(72, 96 * zoom)),
    });
    if (!result) return;

    this.post({ type: 'pages', seq: ++this.seq, patches: result.patches });
  }

  /** Preview → editor. */
  private async jumpFromClick(page: number, xPt: number, yPt: number): Promise<void> {
    const result = await this.client.request<JumpResult | null>(
      'typst/jumpFromClick',
      { page, xPt, yPt },
    );
    if (!result) return;

    if (result.kind === 'url') {
      if (isAllowedLink(result.url)) {
        void vscode.env.openExternal(vscode.Uri.parse(result.url));
      }
      return;
    }
    if (result.kind === 'page') {
      this.post({ type: 'goToPage', page: result.page });
      return;
    }

    const uri = vscode.Uri.parse(result.uri);
    const document = await vscode.workspace.openTextDocument(uri);
    const editor = await vscode.window.showTextDocument(document, {
      preserveFocus: false,
      viewColumn: vscode.ViewColumn.One,
    });

    const position = new vscode.Position(result.position.line, result.position.character);
    this.guard.markPreviewOrigin();
    editor.selection = new vscode.Selection(position, position);
    editor.revealRange(
      new vscode.Range(position, position),
      vscode.TextEditorRevealType.InCenterIfOutsideViewport,
    );
  }

  /** Preview scroll → editor scroll. */
  private async onPreviewScrolled(page: number, yPt: number): Promise<void> {
    const settings = config.read(this.target);
    const mode = settings.host.preview.scrollSync;
    if (mode !== 'both' && mode !== 'previewToEditor') return;
    if (this.guard.isEditorOrigin()) return;

    const result = await this.client.request<JumpResult | null>(
      'typst/jumpFromClick',
      { page, xPt: 20, yPt },
    );
    if (!result || result.kind !== 'source') return;

    const editor = vscode.window.visibleTextEditors.find(
      (candidate) => candidate.document.uri.toString() === result.uri,
    );
    if (!editor) return;

    this.guard.markPreviewOrigin();
    const position = new vscode.Position(result.position.line, 0);
    editor.revealRange(
      new vscode.Range(position, position),
      vscode.TextEditorRevealType.AtTop,
    );
  }

  /** Editor cursor → preview. */
  private onCursorMoved(event: vscode.TextEditorSelectionChangeEvent): void {
    if (!this.panel || event.textEditor.document.languageId !== 'typst') return;

    const settings = config.read(event.textEditor.document.uri);
    const mode = settings.host.preview.scrollSync;
    if (mode !== 'both' && mode !== 'editorToPreview') return;
    if (this.guard.isPreviewOrigin()) return;

    this.guard.debounce(50, () => void this.sendCursor(event.textEditor, false));
  }

  private async sendCursor(editor: vscode.TextEditor, force: boolean): Promise<void> {
    if (!this.panel) return;

    const settings = config.read(editor.document.uri);
    if (!force && !settings.host.preview.cursorIndicator) return;

    const position = editor.selection.active;
    const points = await this.client.request<
      { page: number; xPt: number; yPt: number }[]
    >('typst/jumpFromCursor', {
      uri: editor.document.uri.toString(),
      position: { line: position.line, character: position.character },
    });

    // `jump_from_cursor` returns nothing for a cursor in a comment or on a
    // keyword — leave the preview where it is rather than jumping somewhere
    // arbitrary.
    const first = points?.[0];
    if (!first) return;

    this.guard.markEditorOrigin();
    this.post({ type: 'cursor', page: first.page, xPt: first.xPt, yPt: first.yPt });
  }

  private pushSettings(): void {
    this.post({ type: 'settings', settings: previewSettings(this.target) });
  }

  private post(message: HostToWebview): void {
    void this.panel?.webview.postMessage(message);
  }
}

/** The subset of the configuration the webview acts on. */
export function previewSettings(scope?: vscode.Uri): PreviewSettings {
  const settings = config.read(scope).host.preview;
  return {
    scrollSync: settings.scrollSync,
    cursorIndicator: settings.cursorIndicator,
    invertColors: settings.invertColors,
    background: settings.background,
    renderMode: settings.renderMode,
  };
}

function uriLabel(uri: vscode.Uri | undefined): string {
  return uri ? uri.path.split('/').pop() ?? 'Typst' : 'Typst';
}
