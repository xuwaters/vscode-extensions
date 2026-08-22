import * as vscode from 'vscode';
import { toggleReadOnly } from './config.js';
import { TableCache } from './document/table.js';
import { CsvEditorProvider } from './editor/provider.js';
import { Rainbow } from './text/rainbow.js';

/**
 * CSV Ultra.
 *
 * Two views of one file, and the point is that neither is a second-class one.
 *
 * The *table* is a custom editor over the text document: an Excel-style grid with
 * frozen headers, in-place editing, sorting, row and column selection, and
 * columns and rows you can drag to size. Every edit it makes is a `WorkspaceEdit`
 * on the document, so undo, save, dirty state and a text editor open beside it are
 * VSCode's own and work without this extension doing anything.
 *
 * The *text editor* gets the file coloured by column — the one thing that makes a
 * forty-column CSV readable as text, because the problem was never the characters,
 * it was telling which comma you are between.
 *
 * Nothing here runs until a separated-values file is opened: one custom editor,
 * one decorator, and a parse cache the two share.
 *
 * ## Where everything is
 *
 * The two surfaces are the top-level split, and the two layers underneath them
 * are shared. Dependencies only ever point downwards:
 *
 * | Folder      | What is in it                                                  |
 * | ----------- | -------------------------------------------------------------- |
 * | `editor/`   | The custom editor — the provider, one session per tab, the page, the layout memory. |
 * | `text/`     | The text editor — column colours and the cell in the status bar. |
 * | `document/` | A `TextDocument` read and written as a table: which dialect, the cached parse, and the edits that write back. |
 * | `csv/`      | The *format*: parsing, writing, sniffing, and how a value sorts. No VSCode and no DOM — this layer is imported by the webview too, which is what stops the table on screen and the bytes on disk disagreeing about the file. |
 *
 * Beside them sit the three files both surfaces need: this one, `config.ts`, and
 * `messages.ts` — the host ⇄ webview protocol, which the page imports directly so
 * a change to one side is a type error on the other.
 */
export function activate(context: vscode.ExtensionContext): void {
  const output = vscode.window.createOutputChannel('CSV Ultra');
  // One parse per document version, shared: the grid turns a cell edit into an
  // offset with it, and the decorator colours a screenful with it. Neither can
  // afford to parse the file itself on every scroll.
  const cache = new TableCache();
  const { provider, registration } = CsvEditorProvider.register(context, output, cache);
  const rainbow = new Rainbow(cache);

  const command = (name: string, run: (...args: never[]) => unknown): vscode.Disposable =>
    vscode.commands.registerCommand(`csvUltra.${name}`, run);

  context.subscriptions.push(
    output,
    cache,
    provider,
    registration,
    rainbow,

    command('open', (uri?: vscode.Uri) => provider.open(uri, vscode.ViewColumn.Active)),
    command('openToSide', (uri?: vscode.Uri) => provider.open(uri, vscode.ViewColumn.Beside)),
    command('openInTextEditor', () => provider.openInTextEditor()),

    command('find', () => provider.run('find')),
    command('findNext', () => provider.run('findNext')),
    command('findPrevious', () => provider.run('findPrevious')),
    command('goToRow', () => provider.goToRow()),

    // Not a grid command: it writes a *setting*, and the change comes back to
    // every open tab — this one included — as a `settings` message. Round-
    // tripping it through the page would have flipped one tab and left the rest.
    command('toggleReadOnly', async () => {
      const on = await toggleReadOnly(provider.activeUri());
      void vscode.window.setStatusBarMessage(
        `CSV Ultra: the table is ${on ? 'read-only' : 'editable'}`,
        2000,
      );
    }),

    command('toggleHeaderRow', () => provider.run('toggleHeaderRow')),
    command('toggleWrap', () => provider.run('toggleWrap')),
    command('autoFitColumns', () => provider.run('autoFitColumns')),
    command('resetLayout', () => provider.run('resetLayout')),
    command('rowHeightIncrease', () => provider.run('rowHeightIncrease')),
    command('rowHeightDecrease', () => provider.run('rowHeightDecrease')),
    command('fontSizeIncrease', () => provider.run('fontSizeIncrease')),
    command('fontSizeDecrease', () => provider.run('fontSizeDecrease')),
    command('fontSizeReset', () => provider.run('fontSizeReset')),

    command('sortAscending', () => provider.run('sortAscending')),
    command('sortDescending', () => provider.run('sortDescending')),
    command('clearSort', () => provider.run('clearSort')),
    command('applySort', () => provider.run('applySort')),

    command('insertRowAbove', () => provider.run('insertRowAbove')),
    command('insertRowBelow', () => provider.run('insertRowBelow')),
    command('deleteRows', () => provider.run('deleteRows')),
    command('insertColumnLeft', () => provider.run('insertColumnLeft')),
    command('insertColumnRight', () => provider.run('insertColumnRight')),
    command('deleteColumns', () => provider.run('deleteColumns')),

    command('setDelimiter', () => provider.readWith()),
    command('convertDelimiter', () => provider.convert()),
    command('convertToTsv', () => provider.convert('\t')),
    command('convertToCsv', () => provider.convert(',')),
    command('saveAsTsv', () => provider.saveCopyAs('\t')),
    command('saveAsCsv', () => provider.saveCopyAs(',')),

    command('toggleRainbow', () => {
      const on = rainbow.toggle();
      void vscode.window.setStatusBarMessage(
        `CSV Ultra: rainbow columns ${on ? 'on' : 'off'}`,
        2000,
      );
    }),
    command('showLog', () => output.show(true)),
  );
}

export function deactivate(): void {
  // Everything is a subscription; VSCode disposes them.
}
