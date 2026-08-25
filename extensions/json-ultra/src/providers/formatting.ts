// Whole-document formatting. Order of preference:
//   1. oxfmt, when the project is configured for it (sort applied by the
//      WASM analyzer *before* the text reaches oxfmt, so both features
//      compose);
//   2. the WASM formatter.

import * as vscode from 'vscode';
import type { AnalyzerBridge } from '../analyzer.js';
import { formatOptionsFor, readFormatSortKeys } from '../config.js';
import { formatWithOxfmt, shouldUseOxc } from '../oxc.js';

export class JsonFormattingProvider implements vscode.DocumentFormattingEditProvider {
  constructor(
    private readonly bridge: AnalyzerBridge,
    private readonly log: (message: string) => void,
  ) {}

  async provideDocumentFormattingEdits(
    document: vscode.TextDocument,
    options: vscode.FormattingOptions,
    token: vscode.CancellationToken,
  ): Promise<vscode.TextEdit[]> {
    const sortKeys = readFormatSortKeys(document.uri);
    const formatOptions = formatOptionsFor(document, options, sortKeys);
    const uri = document.uri.toString();
    this.bridge.updateFile(uri, document.getText(), document.languageId);

    if (shouldUseOxc(document)) {
      let text = document.getText();
      if (sortKeys) {
        const sorted = this.bridge.sortKeys(uri, formatOptions);
        if (sorted) text = withDocumentEol(document, sorted.new_text);
      }
      const formatted = await formatWithOxfmt(document, text, this.log);
      if (token.isCancellationRequested) return [];
      if (formatted !== null) {
        if (formatted === document.getText()) return [];
        return [vscode.TextEdit.replace(fullRange(document), formatted)];
      }
      // oxfmt unavailable — fall through to the built-in formatter.
    }

    const edit = this.bridge.formatting(uri, formatOptions);
    if (!edit) return [];
    return [
      vscode.TextEdit.replace(
        new vscode.Range(
          new vscode.Position(edit.start.line, edit.start.col),
          new vscode.Position(edit.end.line, edit.end.col),
        ),
        withDocumentEol(document, edit.new_text),
      ),
    ];
  }
}

export function fullRange(document: vscode.TextDocument): vscode.Range {
  return new vscode.Range(
    new vscode.Position(0, 0),
    document.positionAt(document.getText().length),
  );
}

/** The analyzer always emits LF; mirror the document's EOL on the way out. */
export function withDocumentEol(document: vscode.TextDocument, text: string): string {
  if (document.eol !== vscode.EndOfLine.CRLF) return text;
  return text.replace(/\n/g, '\r\n');
}
