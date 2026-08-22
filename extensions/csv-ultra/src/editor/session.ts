import * as vscode from 'vscode';
import {
  CONFIG_SECTION,
  readGridSettings,
  readMaxFileSize,
  readQuoteStyle,
  readReadOnly,
  readRememberLayout,
} from '../config.js';
import type { Dialect } from '../csv/dialect.js';
import { writeTable } from '../csv/serialize.js';
import { dialectFor } from '../document/dialectFor.js';
import { convertDialect, editsFor, type OffsetEdit } from '../document/edits.js';
import type { TableCache } from '../document/table.js';
import {
  parseWebviewMessage,
  type GridCommand,
  type GridEdit,
  type GridLayout,
  type GridPlace,
  type HostToWebview,
  type LoadReason,
  type WebviewToHost,
} from '../messages.js';
import { html } from './html.js';
import type { LayoutMemory } from './layoutMemory.js';

/** How long a burst of foreign edits settles before the table is rebuilt. */
const SETTLE_MS = 120;

/**
 * Whether an edit moves rows or columns the page cannot have moved itself.
 *
 * A cell written inside the table is already on screen — the page put it there
 * optimistically and the document has caught up. Anything that changes the
 * *shape* of the file, a write past the last record included, comes back as a
 * fresh load rather than being replayed twice in two places.
 */
function reshapes(edit: GridEdit, records: number): boolean {
  if (edit.kind !== 'cells') return true;
  return edit.patches.some((patch) => patch.row >= records);
}

/**
 * One open tab.
 *
 * A tab is bound to its document for its life — VSCode owns these webviews and
 * gives each one file — so there is no retargeting here and no following the
 * active editor. What the session owns is the round trip: the document's text on
 * the way in, and the edits the page asks for on the way out.
 *
 * The direction of that trip is the design. The page never writes text. It says
 * "row 4 column 2 now holds this" and the host turns that into a `WorkspaceEdit`
 * over the document, which means the file's undo stack, its dirty flag, its
 * save, its hot exit and its live sync with a text editor open on the same file
 * are all VSCode's own and none of them are reimplemented here. A grid that owned
 * the bytes would have had to reimplement every one of them, and would have been
 * one crash away from losing somebody's data.
 *
 * The other half of that trip is what happens when the document changes. Our own
 * edits are recognised (`applying`) and skipped, because the page already moved
 * the cell before it asked; anything else — a text editor beside us, a revert, a
 * git checkout — reloads the table from the document, which is the only thing
 * either side treats as the truth.
 */
export class GridSession {
  private readonly disposables: vscode.Disposable[] = [];
  private latest: GridPlace | undefined;
  private settle: ReturnType<typeof setTimeout> | undefined;
  /** Non-zero while a `WorkspaceEdit` of ours is in flight. */
  private applying = 0;
  private disposed = false;
  /** A delimiter chosen for this tab, overriding the configuration. */
  private override: string | undefined;
  private editing = false;
  /** Where to put the reader on the next load. One-shot. */
  private pendingSelect: { row: number; column: number } | undefined;

  /** Fires whenever this tab's cell, size or selection changes. */
  readonly onDidChangePlace = new vscode.EventEmitter<GridSession>();
  /** Fires when a cell editor opens or closes, so the keybindings can stand down. */
  readonly onDidChangeEditing = new vscode.EventEmitter<GridSession>();

  constructor(
    private readonly document: vscode.TextDocument,
    private readonly panel: vscode.WebviewPanel,
    extensionUri: vscode.Uri,
    private readonly output: vscode.OutputChannel,
    private readonly layouts: LayoutMemory,
    private readonly cache: TableCache,
  ) {
    panel.webview.options = { enableScripts: true, localResourceRoots: [extensionUri] };
    panel.webview.html = html(panel.webview, extensionUri);

    this.disposables.push(
      panel.webview.onDidReceiveMessage((raw: unknown) => {
        const message = parseWebviewMessage(raw);
        if (!message) {
          this.output.appendLine('grid: dropped a malformed message');
          return;
        }
        void this.onMessage(message);
      }),
      vscode.workspace.onDidChangeTextDocument((event) => {
        if (event.document !== this.document) return;
        // Our own edit: the page moved the cell before it asked for it, and
        // shipping the whole file back would be a repaint of what is already on
        // screen. A missed flag costs a redundant reload, not a wrong table.
        if (this.applying > 0) return;
        if (event.contentChanges.length === 0) return;
        this.scheduleLoad('external');
      }),
      vscode.workspace.onDidChangeConfiguration((event) => {
        if (!event.affectsConfiguration(CONFIG_SECTION, this.uri)) return;
        // A changed delimiter is a different table, not a restyled one — it has
        // to be re-read from the document. Everything else is a setting the page
        // can apply to the table it already has. A tab that was told which
        // delimiter to use is not listening to the setting any more.
        const reread =
          this.override === undefined &&
          event.affectsConfiguration(`${CONFIG_SECTION}.delimiter`, this.uri);
        if (reread) this.scheduleLoad('delimiter');
        else this.send({ type: 'settings', settings: readGridSettings(this.uri) });
      }),
      panel.onDidChangeViewState(() => {
        if (panel.visible) this.send({ type: 'visible' });
        // Coming back to this tab from the keyboard focuses the page but nothing
        // in it, and the keys that move the selection act on whatever holds the
        // focus. Without this the reader has to click a cell before ↓ moves.
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

  /** Where the reader is, once the page has said. */
  get place(): GridPlace | undefined {
    return this.latest;
  }

  /** Whether a cell editor is open in this tab. */
  get isEditing(): boolean {
    return this.editing;
  }

  /** The dialect this tab reads its document with. */
  dialect(): Dialect {
    return dialectFor(this.document, this.override);
  }

  /** Ask the page to do something — from the title bar, a key, or the palette. */
  command(command: GridCommand): void {
    this.send({ type: 'command', command });
  }

  /** Bring this tab forward. */
  reveal(): void {
    this.panel.reveal(this.panel.viewColumn, false);
  }

  /** The document behind this tab, for the commands that read it directly. */
  get textDocument(): vscode.TextDocument {
    return this.document;
  }

  /**
   * Land the reader on a cell — how a jump from the text editor keeps its place.
   *
   * Parked rather than sent when the page has not loaded yet, because a tab
   * being opened for the first time has no grid to move around in. `load` posts
   * it the moment it has one.
   */
  select(row: number, column: number): void {
    if (this.latest) this.send({ type: 'select', row, column });
    else this.pendingSelect = { row, column };
  }

  /** Read the file as if it were separated by something else. Changes no bytes. */
  readWith(delimiter: string): void {
    this.override = delimiter;
    this.cache.forget(this.uri);
    void this.load('delimiter');
  }

  /**
   * Rewrite the document with a different delimiter.
   *
   * The tab is then pinned to the new delimiter, because the file's *name* may
   * still say otherwise: a `.csv` full of tabs would be sniffed straight back to
   * commas on the next open, and the reader would be looking at one column. Save
   * a copy under the matching extension to make it stick.
   */
  async convert(to: string): Promise<boolean> {
    const dialect = this.dialect();
    if (to === dialect.delimiter) return false;
    const { table, length } = this.cache.of(this.document, dialect);
    const edits = convertDialect(
      table,
      length,
      { ...dialect, delimiter: to },
      readQuoteStyle(this.uri),
    );
    if (edits.length === 0) return false;
    const applied = await this.apply(edits);
    if (applied) this.readWith(to);
    return applied;
  }

  /** The document, written out with a different delimiter, for a save-a-copy. */
  exportWith(to: string): string {
    const dialect = this.dialect();
    const { table } = this.cache.of(this.document, dialect);
    return writeTable(
      table.records,
      { ...dialect, delimiter: to },
      readQuoteStyle(this.uri),
      table.trailingNewline,
    );
  }

  private async onMessage(message: WebviewToHost): Promise<void> {
    switch (message.type) {
      case 'ready':
        await this.load('open');
        break;

      case 'edit': {
        // Read-only is the host's answer as well as the page's. The page guards
        // every gesture that writes, but the page is not the authority on
        // whether this file may be written — and it has already moved the cell
        // on screen, so a refusal has to put the document back over the top.
        if (readReadOnly(this.uri)) {
          await this.load('external');
          break;
        }
        const dialect = this.dialect();
        const { table, length } = this.cache.of(this.document, dialect);
        const edits = editsFor(table, length, message.edit, readQuoteStyle(this.uri));
        if (edits.length === 0) break;
        const reload = reshapes(message.edit, table.records.length);
        if (await this.apply(edits)) {
          // A cell the page already changed on screen needs nothing back. A row
          // inserted, a column removed or a sort written down moved everything
          // below it, and re-deriving that here *and* there would be two
          // implementations of one answer — so the file says what happened.
          if (reload) await this.load('external');
        }
        break;
      }

      case 'place':
        this.latest = message.place;
        this.onDidChangePlace.fire(this);
        break;

      case 'editing':
        if (this.editing === message.editing) break;
        this.editing = message.editing;
        this.onDidChangeEditing.fire(this);
        break;

      case 'clipboard':
        await vscode.env.clipboard.writeText(message.text);
        break;

      case 'requestPaste':
        this.send({ type: 'paste', text: await vscode.env.clipboard.readText() });
        break;

      case 'run':
        // The command list is a closed union, checked on the way in — see
        // `HostCommand`. Nothing the page sends can name a command outside it.
        await vscode.commands.executeCommand(`csvUltra.${message.command}`);
        break;

      case 'error':
        this.output.appendLine(`grid: ${message.context}: ${message.message}`);
        break;
    }
  }

  /**
   * Apply edits to the document.
   *
   * One `WorkspaceEdit` for the whole gesture, so one undo step: a pasted block
   * over fifty rows is fifty splices that come back in one `⌘Z`, which is what
   * anybody who has ever pasted into the wrong place expects.
   */
  private async apply(edits: readonly OffsetEdit[]): Promise<boolean> {
    const workspace = new vscode.WorkspaceEdit();
    for (const edit of edits) {
      workspace.replace(
        this.uri,
        new vscode.Range(
          this.document.positionAt(edit.start),
          this.document.positionAt(edit.end),
        ),
        edit.newText,
      );
    }
    this.applying += 1;
    try {
      const applied = await vscode.workspace.applyEdit(workspace);
      if (!applied) {
        this.output.appendLine('grid: the edit was refused');
        // The page has already moved the cell; put the truth back on screen.
        await this.load('external');
      }
      return applied;
    } finally {
      this.applying -= 1;
    }
  }

  private scheduleLoad(reason: LoadReason): void {
    if (this.settle) clearTimeout(this.settle);
    this.settle = setTimeout(() => {
      this.settle = undefined;
      void this.load(reason);
    }, SETTLE_MS);
  }

  /**
   * Hand the page the document's text.
   *
   * The whole file, every time. A CSV is not a document that can be patched
   * across the boundary — a delimiter typed into a cell changes the shape of a
   * row, a quote changes the shape of everything after it — and there is exactly
   * one correct answer to "what does this file say", which is what the document
   * says. Loads are debounced rather than incremental.
   */
  private async load(reason: LoadReason): Promise<void> {
    if (this.disposed) return;
    const text = this.document.getText();
    const limit = readMaxFileSize(this.uri);
    if (text.length > limit) {
      this.send({ type: 'refused', bytes: text.length, limit });
      return;
    }
    this.send({
      type: 'load',
      name: this.name,
      text,
      dialect: this.dialect(),
      settings: readGridSettings(this.uri),
      reason,
      layout: readRememberLayout(this.uri) ? this.layouts.get(this.uri) : undefined,
    });
    const select = this.pendingSelect;
    this.pendingSelect = undefined;
    if (select) this.send({ type: 'select', ...select });
  }

  /** What the tab and the save dialog call this document. */
  get name(): string {
    return this.uri.path.split('/').pop() ?? 'table.csv';
  }

  private send(message: HostToWebview): void {
    if (this.disposed) return;
    void this.panel.webview.postMessage(message);
  }

  /**
   * Park the layout on the way out. Written on dispose rather than on every
   * drag: the widths only matter to a *later* open, and a write to workspace
   * state per pointer move is a lot of writes for one number.
   */
  async close(): Promise<void> {
    this.disposed = true;
    if (this.settle) clearTimeout(this.settle);
    for (const disposable of this.disposables) disposable.dispose();
    this.disposables.length = 0;
    const layout: GridLayout | undefined = this.latest?.layout;
    this.onDidChangePlace.dispose();
    this.onDidChangeEditing.dispose();
    if (layout && readRememberLayout(this.uri)) {
      await this.layouts.park(this.uri, layout);
    }
  }
}
