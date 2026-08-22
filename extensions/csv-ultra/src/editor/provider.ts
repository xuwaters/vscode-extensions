import * as vscode from 'vscode';
import { readQuoteStyle } from '../config.js';
import { DELIMITER_NAMES, delimiterName, type Dialect } from '../csv/dialect.js';
import { fieldSpans } from '../csv/parse.js';
import { writeTable } from '../csv/serialize.js';
import { columnLabel } from '../csv/values.js';
import { dialectFor } from '../document/dialectFor.js';
import { convertDialect } from '../document/edits.js';
import { columnAt, recordIndexAt, type TableCache } from '../document/table.js';
import type { GridCommand } from '../messages.js';
import { CSV_LANGUAGES } from '../text/rainbow.js';
import { LayoutMemory } from './layoutMemory.js';
import { GridSession } from './session.js';

/** Set while a CSV Ultra tab is the active one; gates the palette entries. */
export const CTX_ACTIVE = 'csvUltra.active';

/**
 * Set while a cell editor is open in the active tab.
 *
 * The keybindings that would fight a text box stand down on it — `⌘Z` most of
 * all. VSCode forwards every keystroke a webview sees to its own keybinding
 * resolver whatever the page does with the event, so undo *while typing in a
 * cell* would otherwise undo the last committed edit instead of the last
 * character, and a page cannot decline on its own behalf. It can only say so.
 */
export const CTX_EDITING = 'csvUltra.editing';

/** VSCode's built-in text editor, for handing a tab back to the source. */
const TEXT_EDITOR = 'default';

/**
 * VSCode's own Reopen With, which swaps the editor *inside* the active tab.
 * `vscode.openWith` cannot: its resolver only reuses a tab when the editor type
 * matches, so opening the source over the grid leaves the grid tab sitting
 * behind it. Replacing keeps the tab's place in the tab bar and hands the unsaved
 * changes over — closing the grid instead would ask to save them.
 */
const REOPEN_ACTIVE_EDITOR_WITH = 'reopenActiveEditorWith';

/**
 * The table, as a custom editor over a text document.
 *
 * `CustomTextEditorProvider` rather than `CustomEditorProvider`, and that is the
 * single most consequential decision in this extension. A CSV *is* text, and
 * declaring it so hands VSCode the whole of the document's life: the undo stack
 * a cell edit lands on, the dirty dot on the tab, `⌘S`, hot exit, revert, the
 * diff view, and a text editor open on the same file in the next group staying in
 * step keystroke by keystroke. A binary custom editor would have owned the bytes
 * and had to reimplement all of it.
 *
 * Registered at `priority: "default"` because for most people most of the time
 * the table is what they wanted from a `.csv`. The text editor is one click away
 * in the title bar and takes the tab over rather than opening a second one — and
 * it is not a lesser view: the rainbow colours in it are the other half of this
 * extension.
 */
export class CsvEditorProvider
  implements vscode.CustomTextEditorProvider, vscode.Disposable
{
  static readonly viewType = 'csvUltra.editor';

  private readonly sessions = new Set<GridSession>();
  private readonly statusBar: vscode.StatusBarItem;
  private readonly layouts: LayoutMemory;
  private readonly disposables: vscode.Disposable[] = [];
  /** Cells parked by `open`, taken by the tab that opens for them. */
  private readonly parked = new Map<string, { row: number; column: number }>();

  constructor(
    private readonly context: vscode.ExtensionContext,
    private readonly output: vscode.OutputChannel,
    private readonly cache: TableCache,
  ) {
    this.layouts = new LayoutMemory(context.workspaceState);
    this.statusBar = vscode.window.createStatusBarItem(
      'csvUltra.place',
      vscode.StatusBarAlignment.Right,
      97,
    );
    this.statusBar.name = 'CSV Ultra Cell';
    this.statusBar.command = 'csvUltra.goToRow';
    this.statusBar.tooltip = 'Go to a row';
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
    cache: TableCache,
  ): { provider: CsvEditorProvider; registration: vscode.Disposable } {
    const provider = new CsvEditorProvider(context, output, cache);
    const registration = vscode.window.registerCustomEditorProvider(
      CsvEditorProvider.viewType,
      provider,
      {
        // A hundred thousand rows of laid-out grid is expensive to rebuild, and
        // a reader flipping between two tabs should not pay for it twice.
        webviewOptions: { retainContextWhenHidden: true },
        supportsMultipleEditorsPerDocument: true,
      },
    );
    return { provider, registration };
  }

  resolveCustomTextEditor(document: vscode.TextDocument, panel: vscode.WebviewPanel): void {
    const session = new GridSession(
      document,
      panel,
      this.context.extensionUri,
      this.output,
      this.layouts,
      this.cache,
    );
    this.sessions.add(session);

    const parked = this.parked.get(document.uri.toString());
    if (parked) {
      this.parked.delete(document.uri.toString());
      session.select(parked.row, parked.column);
    }

    const place = session.onDidChangePlace.event(() => this.refresh());
    const editing = session.onDidChangeEditing.event(() => this.refresh());
    const viewState = panel.onDidChangeViewState(() => this.refresh());
    panel.onDidDispose(() => {
      place.dispose();
      editing.dispose();
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
  private target(): GridSession | undefined {
    for (const session of this.sessions) if (session.active) return session;
    return this.sessions.size === 1 ? [...this.sessions][0] : undefined;
  }

  /**
   * The file a resource-scoped setting should be read and written for: the tab
   * in front, or the text editor holding the same kind of file when there is no
   * table open at all.
   */
  activeUri(): vscode.Uri | undefined {
    return this.target()?.uri ?? this.activeDocument()?.uri;
  }

  /** Run a grid command against the tab in front of the reader. */
  run(command: GridCommand): void {
    const session = this.target();
    if (!session) return this.noTable();
    session.command(command);
  }

  /**
   * Ask for a row number — in the table's own row box, not in a quick pick over
   * the top of it.
   *
   * The box is already on screen, already showing the row the reader is on, and
   * already knows what to do with a number typed into it. The tab comes forward
   * first, because this is also the status bar's command and a webview that is
   * not in front cannot take focus.
   */
  goToRow(): void {
    const session = this.target();
    if (!session) return this.noTable();
    session.reveal();
    session.command('goToRow');
  }

  /**
   * Open a file in the table.
   *
   * When the reader is looking at the same file as text, the cell their cursor is
   * in is parked for the tab that is about to open — landing in the table at the
   * top of a fifty-thousand-row file they were half way down is not opening the
   * same file.
   */
  async open(uri: vscode.Uri | undefined, column: vscode.ViewColumn): Promise<void> {
    const target = uri ?? this.activeDocument()?.uri;
    if (!target) return this.noTable();

    const editor = vscode.window.visibleTextEditors.find(
      (candidate) => candidate.document.uri.toString() === target.toString(),
    );
    if (editor) {
      const cell = this.cellAt(editor);
      if (cell) this.parked.set(target.toString(), cell);
    }

    await vscode.commands.executeCommand('vscode.openWith', target, CsvEditorProvider.viewType, {
      viewColumn: column,
      preserveFocus: false,
    });
  }

  /**
   * Hand the active tab back to the text editor, with the cursor in the cell the
   * reader was on.
   */
  async openInTextEditor(): Promise<void> {
    const session = this.target();
    if (!session) return this.noTable();
    const place = session.place;
    session.reveal();
    await vscode.commands.executeCommand(REOPEN_ACTIVE_EDITOR_WITH, TEXT_EDITOR);
    if (!place) return;

    const document = session.textDocument;
    const dialect = session.dialect();
    const { table } = this.cache.of(document, dialect);
    const record = table.records[place.row - 1];
    if (!record) return;
    const span = fieldSpans(
      document.getText(
        new vscode.Range(document.positionAt(record.start), document.positionAt(record.end)),
      ),
      dialect,
    )[place.column];
    const position = document.positionAt(record.start + (span?.start ?? 0));

    const editor = vscode.window.visibleTextEditors.find(
      (candidate) => candidate.document.uri.toString() === document.uri.toString(),
    );
    if (!editor) return;
    editor.selection = new vscode.Selection(position, position);
    editor.revealRange(new vscode.Range(position, position), vscode.TextEditorRevealType.InCenter);
  }

  /** Read the active table as if it were separated by something else. */
  async readWith(): Promise<void> {
    const session = this.target();
    if (!session) return this.noTable();
    const current = session.dialect().delimiter;
    const choice = await this.pickDelimiter('Read this file with', current);
    if (choice === undefined) return;
    session.readWith(choice);
  }

  /**
   * Convert the file to a different delimiter, in place.
   *
   * Works from either view — the table or the text editor — because "this is a
   * CSV and I wanted a TSV" is a thought people have with the source in front of
   * them at least as often.
   */
  async convert(to?: string): Promise<void> {
    const session = this.target();
    const document = session?.textDocument ?? this.activeDocument();
    if (!document) return this.noTable();

    const dialect = session?.dialect() ?? this.dialectOf(document);
    const target = to ?? (await this.pickDelimiter('Convert this file to', dialect.delimiter));
    if (target === undefined) return;
    if (target === dialect.delimiter) {
      void vscode.window.showInformationMessage(
        `CSV Ultra: this file is already ${delimiterName(target).toLowerCase()}-separated.`,
      );
      return;
    }

    if (session) {
      if (await session.convert(target)) this.announceConversion(document, target);
      return;
    }

    const { table, length } = this.cache.of(document, dialect);
    const edits = convertDialect(
      table,
      length,
      { ...dialect, delimiter: target },
      readQuoteStyle(document.uri),
    );
    if (edits.length === 0) return;
    const workspace = new vscode.WorkspaceEdit();
    for (const edit of edits) {
      workspace.replace(
        document.uri,
        new vscode.Range(document.positionAt(edit.start), document.positionAt(edit.end)),
        edit.newText,
      );
    }
    if (await vscode.workspace.applyEdit(workspace)) {
      this.announceConversion(document, target);
    }
  }

  /**
   * Write the file out under a different delimiter, leaving the original alone.
   *
   * The companion to converting in place, and the one that matters more often: a
   * `.csv` full of tabs is a file every other tool will read wrong, so the
   * conversion that ends in a `.tsv` on disk is usually the one that was meant.
   */
  async saveCopyAs(to: string): Promise<void> {
    const session = this.target();
    const document = session?.textDocument ?? this.activeDocument();
    if (!document) return this.noTable();

    const dialect = session?.dialect() ?? this.dialectOf(document);
    const body = session
      ? session.exportWith(to)
      : ((): string => {
          const { table } = this.cache.of(document, dialect);
          return writeTable(
            table.records,
            { ...dialect, delimiter: to },
            readQuoteStyle(document.uri),
            table.trailingNewline,
          );
        })();

    const suffix = to === '\t' ? 'tsv' : to === '|' ? 'psv' : 'csv';
    const stem = (document.uri.path.split('/').pop() ?? 'table').replace(
      /\.(csv|tsv|tab|psv|txt)$/i,
      '',
    );
    const destination = await vscode.window.showSaveDialog({
      defaultUri: vscode.Uri.joinPath(document.uri, '..', `${stem}.${suffix}`),
      filters: { 'Separated values': ['csv', 'tsv', 'tab', 'psv'] },
      title: `Save a copy as ${delimiterName(to).toLowerCase()}-separated`,
    });
    if (!destination) return;

    try {
      await vscode.workspace.fs.writeFile(destination, Buffer.from(body, 'utf8'));
    } catch (error) {
      void vscode.window.showErrorMessage(
        `CSV Ultra: could not write the copy: ${describe(error)}`,
      );
      return;
    }
    const open = await vscode.window.showInformationMessage(
      `Wrote ${destination.path.split('/').pop()}`,
      'Open',
    );
    if (open === 'Open') await this.open(destination, vscode.ViewColumn.Active);
  }

  /** The document a command should act on when no table tab is in front. */
  private activeDocument(): vscode.TextDocument | undefined {
    const document = vscode.window.activeTextEditor?.document;
    if (!document) return undefined;
    if (CSV_LANGUAGES.has(document.languageId)) return document;
    return /\.(csv|tsv|tab|psv)$/i.test(document.uri.path) ? document : undefined;
  }

  private dialectOf(document: vscode.TextDocument): Dialect {
    return dialectFor(document);
  }

  /** Which cell a text editor's cursor is in, as file coordinates. */
  private cellAt(editor: vscode.TextEditor): { row: number; column: number } | undefined {
    const { document } = editor;
    const dialect = this.dialectOf(document);
    const { table } = this.cache.of(document, dialect);
    const offset = document.offsetAt(editor.selection.active);
    const index = recordIndexAt(table, offset);
    if (index < 0) return undefined;
    const record = table.records[index]!;
    const text = document.getText(
      new vscode.Range(document.positionAt(record.start), document.positionAt(record.end)),
    );
    return { row: index, column: columnAt(text, offset - record.start, dialect) };
  }

  private async pickDelimiter(title: string, current: string): Promise<string | undefined> {
    const picked = await vscode.window.showQuickPick(
      DELIMITER_NAMES.map((entry) => ({
        label: entry.label,
        description: entry.value === current ? 'current' : describeCharacter(entry.value),
        value: entry.value,
      })),
      { title, placeHolder: 'Delimiter' },
    );
    return picked?.value;
  }

  /**
   * Say what happened, and why the file's *name* may now be lying.
   *
   * A `.csv` holding tabs is read back as one column by everything that goes by
   * the extension — this extension included, on the next open — so the message
   * offers the thing that actually finishes the job.
   */
  private announceConversion(document: vscode.TextDocument, to: string): void {
    const expected = to === '\t' ? '.tsv' : to === '|' ? '.psv' : '.csv';
    const name = document.uri.path.split('/').pop() ?? '';
    if (name.toLowerCase().endsWith(expected)) {
      void vscode.window.showInformationMessage(
        `CSV Ultra: converted to ${delimiterName(to).toLowerCase()}-separated.`,
      );
      return;
    }
    void vscode.window
      .showWarningMessage(
        `Converted to ${delimiterName(to).toLowerCase()}-separated, but ${name} does not end in ${expected} — other tools will read it wrong.`,
        'Save a Copy…',
      )
      .then((choice) => {
        if (choice === 'Save a Copy…') void this.saveCopyAs(to);
      });
  }

  private noTable(): void {
    void vscode.window.showInformationMessage('CSV Ultra: open a CSV or TSV file first.');
  }

  private refresh(): void {
    const session = this.target();
    const active = [...this.sessions].some((candidate) => candidate.active);
    void vscode.commands.executeCommand('setContext', CTX_ACTIVE, active);
    void vscode.commands.executeCommand(
      'setContext',
      CTX_EDITING,
      session?.isEditing === true,
    );

    const place = session?.place;
    if (!session || !place || place.rows === 0) {
      this.statusBar.hide();
      return;
    }
    const cell = `R${place.row} · ${columnLabel(place.column)}`;
    const named = place.columnName ? `${cell} ${place.columnName}` : cell;
    const selection = place.selected > 1 ? ` · ${place.selected.toLocaleString()} cells` : '';
    this.statusBar.text = `$(table) ${named} · ${place.rows.toLocaleString()} × ${place.columns}${selection}`;
    this.statusBar.show();
  }

  dispose(): void {
    for (const disposable of this.disposables) disposable.dispose();
    this.disposables.length = 0;
  }
}

function describeCharacter(value: string): string {
  return value === '\t' ? '\\t' : value;
}

function describe(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
