import * as vscode from 'vscode';
import { readRainbow, readStatusBar } from '../config.js';
import type { Dialect } from '../csv/dialect.js';
import { columnLabel } from '../csv/values.js';
import { dialectFor } from '../document/dialectFor.js';
import { columnAt, recordIndexAt, type TableCache } from '../document/table.js';
import { paintPlan, RAINBOW_COLORS, type ColorSpan } from './paint.js';

/** The language ids this extension contributes, and the only ones it paints. */
export const CSV_LANGUAGES = new Set(['csv', 'tsv', 'psv']);

/** Records painted either side of what is on screen, so a small scroll is instant. */
const OVERSCAN_RECORDS = 200;

/** How long a burst of scrolling or typing settles before the paint. */
const SETTLE_MS = 40;

/**
 * Column colours in the *text* editor, and the cell the cursor is in.
 *
 * The grid is one way to read a CSV; the other is the way it actually is, and a
 * file of forty unlabelled columns is unreadable as text for one reason — you
 * cannot tell which comma you are between. Colour fixes exactly that, and it is
 * the reason this extension does not simply claim every `.csv` and hide the
 * source.
 *
 * The colours are *theme* colours (`csvUltra.column1` … `column10`) rather than
 * literals, which buys three things: per-theme defaults for dark, light and both
 * high-contrast themes; `workbench.colorCustomizations` as the override, so a
 * reader edits their colours where they edit every other colour; and the same
 * values reaching the webview, where VSCode publishes every registered colour as
 * a `--vscode-…` custom property. The grid's column headers are tinted from that
 * — so a column is the same colour in both views of the same file.
 *
 * Only what is on screen is painted, plus a couple of hundred records either
 * side. A hundred-thousand-line file has no business owning a million
 * decorations to show fifty rows.
 */
export class Rainbow implements vscode.Disposable {
  private readonly types: vscode.TextEditorDecorationType[] = [];
  private readonly statusBar: vscode.StatusBarItem;
  private readonly disposables: vscode.Disposable[] = [];
  private settle: ReturnType<typeof setTimeout> | undefined;
  /** A session-scoped override of the setting, from the toggle command. */
  private override: boolean | undefined;

  constructor(private readonly cache: TableCache) {
    for (let index = 0; index < RAINBOW_COLORS; index += 1) {
      this.types.push(
        vscode.window.createTextEditorDecorationType({
          color: new vscode.ThemeColor(`csvUltra.column${index + 1}`),
          // Without this, typing at the end of a coloured field extends the
          // decoration over the delimiter and into the next column, and the
          // colours creep rightwards until the next repaint catches up.
          rangeBehavior: vscode.DecorationRangeBehavior.ClosedClosed,
        }),
      );
    }

    this.statusBar = vscode.window.createStatusBarItem(
      'csvUltra.cell',
      vscode.StatusBarAlignment.Right,
      96,
    );
    this.statusBar.name = 'CSV Ultra Cell';
    this.statusBar.command = 'csvUltra.open';
    this.statusBar.tooltip = 'Open this file in CSV Ultra';

    this.disposables.push(
      this.statusBar,
      vscode.window.onDidChangeActiveTextEditor(() => this.schedule()),
      vscode.window.onDidChangeTextEditorVisibleRanges((event) => {
        if (event.textEditor === vscode.window.activeTextEditor) this.schedule();
      }),
      vscode.window.onDidChangeTextEditorSelection((event) => {
        if (event.textEditor === vscode.window.activeTextEditor) this.schedule();
      }),
      vscode.workspace.onDidChangeTextDocument((event) => {
        if (event.document === vscode.window.activeTextEditor?.document) this.schedule();
      }),
      vscode.workspace.onDidCloseTextDocument((document) => this.cache.forget(document.uri)),
      vscode.workspace.onDidChangeConfiguration((event) => {
        if (!event.affectsConfiguration('csvUltra')) return;
        // A setting changed under a session toggle: the setting is the newer
        // instruction, so the toggle steps aside.
        this.override = undefined;
        this.repaintAll();
      }),
    );

    this.schedule();
  }

  /** Flip the colours for this session, without writing to the settings. */
  toggle(): boolean {
    const editor = vscode.window.activeTextEditor;
    const configured = readRainbow(editor?.document.uri).enabled;
    this.override = !(this.override ?? configured);
    this.repaintAll();
    return this.override;
  }

  /** Repaint everything, after a settings change or a toggle. */
  private repaintAll(): void {
    for (const editor of vscode.window.visibleTextEditors) this.clear(editor);
    this.schedule();
  }

  private schedule(): void {
    if (this.settle) clearTimeout(this.settle);
    this.settle = setTimeout(() => {
      this.settle = undefined;
      this.paint();
    }, SETTLE_MS);
  }

  private paint(): void {
    const editor = vscode.window.activeTextEditor;
    if (!editor) {
      this.statusBar.hide();
      return;
    }
    const { document } = editor;
    if (!CSV_LANGUAGES.has(document.languageId)) {
      this.statusBar.hide();
      return;
    }

    const settings = readRainbow(document.uri);
    const enabled = this.override ?? settings.enabled;

    // `document.getText().length` would be a copy of the whole file just to
    // measure it; the last position is the same number and costs nothing.
    const characters = document.offsetAt(
      document.lineAt(document.lineCount - 1).range.end,
    );
    if (!enabled || characters > settings.maxBytes) {
      this.clear(editor);
      this.showCell(editor, undefined);
      return;
    }

    const dialect = dialectFor(document);
    const { table } = this.cache.of(document, dialect);

    if (table.columns <= 1) {
      this.clear(editor);
      this.showCell(editor, dialect);
      return;
    }

    const buckets: ColorSpan[][] = Array.from({ length: RAINBOW_COLORS }, () => []);
    for (const range of editor.visibleRanges) {
      const from = recordIndexAt(table, document.offsetAt(range.start)) - OVERSCAN_RECORDS;
      const to = recordIndexAt(table, document.offsetAt(range.end)) + OVERSCAN_RECORDS;
      const plan = paintPlan(table, from, to, (index) => {
        const record = table.records[index]!;
        return document.getText(
          new vscode.Range(document.positionAt(record.start), document.positionAt(record.end)),
        );
      });
      for (let color = 0; color < RAINBOW_COLORS; color += 1) {
        buckets[color]!.push(...plan[color]!);
      }
    }

    for (let color = 0; color < RAINBOW_COLORS; color += 1) {
      editor.setDecorations(
        this.types[color]!,
        buckets[color]!.map(
          (span) =>
            new vscode.Range(document.positionAt(span.start), document.positionAt(span.end)),
        ),
      );
    }

    this.showCell(editor, dialect);
  }

  /**
   * Name the cell the cursor is in.
   *
   * "Column 7" of a wide file is not an answer anybody wanted, so the header's
   * own word for it comes along when the file has one — which is the fastest way
   * to read a CSV as text that this extension has.
   */
  private showCell(editor: vscode.TextEditor, dialect: Dialect | undefined): void {
    if (!dialect || !readStatusBar(editor.document.uri)) {
      this.statusBar.hide();
      return;
    }
    const { document } = editor;
    const { table } = this.cache.of(document, dialect);
    const offset = document.offsetAt(editor.selection.active);
    const index = recordIndexAt(table, offset);
    if (index < 0) {
      this.statusBar.hide();
      return;
    }

    const record = table.records[index]!;
    const text = document.getText(
      new vscode.Range(document.positionAt(record.start), document.positionAt(record.end)),
    );
    const column = columnAt(text, offset - record.start, dialect);
    const header = table.records[0];
    const name =
      index > 0 && header ? (header.fields[column]?.value ?? '').trim() : '';

    this.statusBar.text = name
      ? `$(table) R${index + 1} · ${columnLabel(column)} ${trim(name)}`
      : `$(table) R${index + 1} · ${columnLabel(column)}`;
    this.statusBar.show();
  }

  private clear(editor: vscode.TextEditor): void {
    for (const type of this.types) editor.setDecorations(type, []);
  }

  dispose(): void {
    if (this.settle) clearTimeout(this.settle);
    for (const type of this.types) type.dispose();
    for (const disposable of this.disposables) disposable.dispose();
    this.disposables.length = 0;
  }
}

function trim(name: string): string {
  return name.length > 24 ? `${name.slice(0, 23)}…` : name;
}
