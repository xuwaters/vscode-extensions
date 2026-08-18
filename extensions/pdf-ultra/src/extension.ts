import * as vscode from 'vscode';
import { PdfEditorProvider } from './provider.js';

/**
 * PDF Ultra.
 *
 * A PDF viewer as a first-class editor: pdf.js rendering into a continuously
 * scrolling column of pages, with a text layer for selection, find and screen
 * readers, an outline, and a reload that keeps the reader's place when the file
 * is rebuilt beneath them.
 *
 * The activation surface is one custom editor and a set of commands that all
 * act on the tab in front of the reader — there is no language server, no
 * background process, and nothing runs until a PDF is opened.
 */
export function activate(context: vscode.ExtensionContext): void {
  const output = vscode.window.createOutputChannel('PDF Ultra');
  const { provider, registration } = PdfEditorProvider.register(context, output);

  const command = (name: string, run: (...args: never[]) => unknown): vscode.Disposable =>
    vscode.commands.registerCommand(`pdfUltra.${name}`, run);

  context.subscriptions.push(
    output,
    provider,
    registration,

    command('open', (uri?: vscode.Uri) => provider.open(uri, vscode.ViewColumn.Active)),
    command('openToSide', (uri?: vscode.Uri) => provider.open(uri, vscode.ViewColumn.Beside)),

    command('nextPage', () => provider.run('nextPage')),
    command('previousPage', () => provider.run('previousPage')),
    command('goToPage', () => provider.goToPage()),

    command('zoomIn', () => provider.run('zoomIn')),
    command('zoomOut', () => provider.run('zoomOut')),
    command('zoomReset', () => provider.run('zoomReset')),
    command('fitWidth', () => provider.run('fitWidth')),
    command('fitPage', () => provider.run('fitPage')),
    command('fitHeight', () => provider.run('fitHeight')),

    command('singlePage', () => provider.run('singlePage')),
    command('continuousPages', () => provider.run('continuousPages')),

    command('rotateClockwise', () => provider.run('rotateClockwise')),
    command('rotateCounterclockwise', () => provider.run('rotateCounterclockwise')),

    command('toggleOutline', () => provider.run('toggleOutline')),
    command('toggleInvertColors', () => provider.run('toggleInvertColors')),
    command('find', () => provider.run('find')),
    command('exportPagePng', () => provider.run('exportPagePng')),

    command('reload', () => provider.reload()),
    command('openInDefaultApp', (uri?: vscode.Uri) => provider.openExternally(uri)),
    command('showLog', () => output.show(true)),
  );
}

export function deactivate(): void {
  // Everything is a subscription; VSCode disposes them.
}
