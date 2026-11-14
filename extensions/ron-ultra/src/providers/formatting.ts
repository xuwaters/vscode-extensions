import * as vscode from 'vscode';
import type { AnalyzerBridge } from '../analyzer';
import { readFormatOptions } from '../config';

export class RonFormattingProvider implements vscode.DocumentFormattingEditProvider {
  constructor(private readonly bridge: AnalyzerBridge) {}

  provideDocumentFormattingEdits(
    document: vscode.TextDocument,
    options: vscode.FormattingOptions,
  ): vscode.TextEdit[] {
    const formatOptions = readFormatOptions(document, options);
    if (!formatOptions) return [];
    const uri = document.uri.toString();
    this.bridge.updateFile(uri, document.getText());
    const edit = this.bridge.formatting(uri, formatOptions);
    if (!edit) return [];
    const range = new vscode.Range(edit.start.line, edit.start.col, edit.end.line, edit.end.col);
    // The analyzer already writes the document's own line endings.
    return [vscode.TextEdit.replace(range, edit.new_text)];
  }
}
