import * as vscode from 'vscode';
import { PdfDocument } from './document.js';
import type { ViewerCommand } from './messages.js';
import { PlaceMemory } from './placeMemory.js';
import { ViewerSession } from './session.js';

/** Set while a PDF Ultra tab is the active one; gates the palette entries. */
export const CTX_ACTIVE = 'pdfUltra.active';

/**
 * The PDF viewer, as a custom editor.
 *
 * Read-only by construction: `CustomReadonlyEditorProvider` gives VSCode no
 * save path to call, so nothing this extension does can write to the document
 * it is showing. Exporting a page writes a *new* file, through a save dialog.
 *
 * Registered at `priority: "default"`, unlike the preview editors in this repo,
 * because the alternative is not a text editor — it is VSCode's "the file is
 * binary" placeholder. Claiming `*.pdf` takes nothing away from anybody, and
 * Reopen With is still there for a reader who wants a different viewer.
 *
 * The provider also owns the two things that are about the *window* rather than
 * about one tab: which tab commands act on, and the page counter in the status
 * bar.
 */
export class PdfEditorProvider
  implements vscode.CustomReadonlyEditorProvider<PdfDocument>, vscode.Disposable
{
  static readonly viewType = 'pdfUltra.editor';

  private readonly sessions = new Set<ViewerSession>();
  private readonly statusBar: vscode.StatusBarItem;
  private readonly places: PlaceMemory;
  private readonly disposables: vscode.Disposable[] = [];

  constructor(
    private readonly context: vscode.ExtensionContext,
    private readonly output: vscode.OutputChannel,
  ) {
    this.places = new PlaceMemory(context.workspaceState);
    this.statusBar = vscode.window.createStatusBarItem(
      'pdfUltra.page',
      vscode.StatusBarAlignment.Right,
      97,
    );
    this.statusBar.name = 'PDF Ultra Page';
    this.statusBar.command = 'pdfUltra.goToPage';
    this.statusBar.tooltip = 'Go to a page';
    this.disposables.push(
      this.statusBar,
      // A custom-editor tab is not a text editor, so activating one is only
      // visible as a tab change.
      vscode.window.tabGroups.onDidChangeTabs(() => this.refresh()),
      vscode.window.tabGroups.onDidChangeTabGroups(() => this.refresh()),
    );
  }

  static register(
    context: vscode.ExtensionContext,
    output: vscode.OutputChannel,
  ): { provider: PdfEditorProvider; registration: vscode.Disposable } {
    const provider = new PdfEditorProvider(context, output);
    const registration = vscode.window.registerCustomEditorProvider(
      PdfEditorProvider.viewType,
      provider,
      {
        // A rasterized 400-page document is expensive to rebuild, and a reader
        // flipping between two tabs should not pay for it twice.
        webviewOptions: { retainContextWhenHidden: true },
        supportsMultipleEditorsPerDocument: true,
      },
    );
    return { provider, registration };
  }

  openCustomDocument(uri: vscode.Uri): PdfDocument {
    return PdfDocument.open(uri);
  }

  resolveCustomEditor(document: PdfDocument, panel: vscode.WebviewPanel): void {
    const session = new ViewerSession(
      document,
      panel,
      this.context.extensionUri,
      this.output,
      this.places,
    );
    this.sessions.add(session);

    const place = session.onDidChangePlace.event(() => this.refresh());
    const viewState = panel.onDidChangeViewState(() => this.refresh());
    panel.onDidDispose(() => {
      place.dispose();
      viewState.dispose();
      this.sessions.delete(session);
      void session.close().then(() => this.refresh());
    });
    this.refresh();
  }

  /**
   * The tab a command acts on: the active one, or — when the palette was opened
   * from somewhere else entirely — the only one open, if there is only one.
   */
  private target(): ViewerSession | undefined {
    for (const session of this.sessions) if (session.active) return session;
    return this.sessions.size === 1 ? [...this.sessions][0] : undefined;
  }

  /** Run a viewer command against the tab in front of the reader. */
  run(command: ViewerCommand): void {
    const session = this.target();
    if (!session) return this.noDocument();
    session.command(command);
  }

  /** Re-read the file behind the tab in front of the reader. */
  async reload(): Promise<void> {
    const session = this.target();
    if (!session) return this.noDocument();
    await session.reloadNow();
  }

  /** Ask for a page number, then go there. */
  async goToPage(): Promise<void> {
    const session = this.target();
    if (!session) return this.noDocument();
    const count = session.pages;
    const answer = await vscode.window.showInputBox({
      title: 'PDF Ultra',
      prompt: count > 0 ? `Page number (1–${count})` : 'Page number',
      value: String(session.page ?? 1),
      validateInput: (text) => {
        const page = Number(text);
        if (!Number.isInteger(page) || page < 1) return 'Enter a page number.';
        if (count > 0 && page > count) return `This document has ${count} pages.`;
        return undefined;
      },
    });
    if (answer === undefined) return;
    session.command('goToPage', Number(answer));
    session.reveal();
  }

  /** Open a PDF in this viewer, by name rather than by association. */
  async open(uri: vscode.Uri | undefined, column: vscode.ViewColumn): Promise<void> {
    const target = uri ?? this.target()?.uri;
    if (!target) return this.noDocument();
    await vscode.commands.executeCommand('vscode.openWith', target, PdfEditorProvider.viewType, {
      viewColumn: column,
      preserveFocus: false,
    });
  }

  /** Hand a document to whatever the operating system opens PDFs with. */
  async openExternally(uri: vscode.Uri | undefined): Promise<void> {
    const target = uri ?? this.target()?.uri;
    if (!target) return this.noDocument();
    await vscode.env.openExternal(target);
  }

  private noDocument(): void {
    void vscode.window.showInformationMessage('PDF Ultra: open a PDF first.');
  }

  private refresh(): void {
    const session = this.target();
    const active = this.sessions.size > 0 && [...this.sessions].some((s) => s.active);
    void vscode.commands.executeCommand('setContext', CTX_ACTIVE, active);

    if (!session || session.pages === 0) {
      this.statusBar.hide();
      return;
    }
    this.statusBar.text = `$(file-pdf) ${session.page ?? 1} / ${session.pages}`;
    this.statusBar.show();
  }

  dispose(): void {
    for (const disposable of this.disposables) disposable.dispose();
    this.disposables.length = 0;
  }
}
