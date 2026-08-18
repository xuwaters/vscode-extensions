import * as vscode from 'vscode';

/** A build rewrites a PDF in several writes; this is how long we wait for the last. */
const SETTLE_MS = 250;

/**
 * One open PDF.
 *
 * VSCode's `CustomDocument` is the per-file half of a custom editor — one
 * instance per file, however many tabs show it. There is no model to hold here:
 * the bytes go straight from disk to the webview through VSCode's resource
 * server, so what this owns is the *watch*.
 *
 * Watching is the point of the class. A PDF is usually an output — LaTeX,
 * Typst, a report generator — and the reader is looking at it beside the thing
 * that produces it. Reload-on-change is what makes the viewer part of that loop
 * instead of a thing to close and reopen. Writes are debounced because a
 * producer rewrites a file in several of them, and rendering a half-written PDF
 * shows an error the next write would have fixed.
 */
export class PdfDocument implements vscode.CustomDocument {
  private readonly changed = new vscode.EventEmitter<void>();
  private readonly deleted = new vscode.EventEmitter<void>();
  private watcher: vscode.FileSystemWatcher | undefined;
  private settle: ReturnType<typeof setTimeout> | undefined;
  /** Bumped on every reload, so the resource URL is never served from cache. */
  private revision = 0;

  /** The file was rewritten and has settled. */
  readonly onDidChange = this.changed.event;
  /** The file went away. */
  readonly onDidDelete = this.deleted.event;

  private constructor(readonly uri: vscode.Uri) {}

  static open(uri: vscode.Uri): PdfDocument {
    const document = new PdfDocument(uri);
    document.watch();
    return document;
  }

  /** The counter a cache-busting query parameter is built from. */
  get version(): number {
    return this.revision;
  }

  /** What the tab and the save dialog call this document. */
  get name(): string {
    return this.uri.path.split('/').pop() ?? 'document.pdf';
  }

  private watch(): void {
    // Only real files have a watcher worth having. A document from a virtual
    // file-system provider is delivered as bytes and does not change under us.
    if (this.uri.scheme !== 'file') return;
    const directory = vscode.Uri.joinPath(this.uri, '..');
    const pattern = new vscode.RelativePattern(directory, this.name);
    this.watcher = vscode.workspace.createFileSystemWatcher(pattern);
    this.watcher.onDidChange(() => this.queueChange());
    // A rewrite that goes through a temporary file lands as delete-then-create,
    // so a create is a change too — not the end of the document.
    this.watcher.onDidCreate(() => this.queueChange());
    this.watcher.onDidDelete(() => {
      if (this.settle) clearTimeout(this.settle);
      this.settle = undefined;
      this.deleted.fire();
    });
  }

  private queueChange(): void {
    if (this.settle) clearTimeout(this.settle);
    this.settle = setTimeout(() => {
      this.settle = undefined;
      void this.fireIfReadable();
    }, SETTLE_MS);
  }

  /**
   * Announce the change, unless the file is currently unreadable or empty —
   * which is what the middle of somebody else's write looks like. The next
   * write brings its own event, so skipping this one loses nothing.
   */
  private async fireIfReadable(): Promise<void> {
    try {
      const stat = await vscode.workspace.fs.stat(this.uri);
      if (stat.size === 0) return;
    } catch {
      return;
    }
    this.revision += 1;
    this.changed.fire();
  }

  dispose(): void {
    if (this.settle) clearTimeout(this.settle);
    this.watcher?.dispose();
    this.changed.dispose();
    this.deleted.dispose();
  }
}
