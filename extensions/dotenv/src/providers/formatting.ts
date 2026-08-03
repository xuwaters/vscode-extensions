import * as vscode from 'vscode';
import type { AnalyzerBridge } from '../analyzer';
import type { AnalyzerTextEdit, FormatOptions } from '../types';

export class DotenvFormattingProvider
  implements vscode.DocumentFormattingEditProvider, vscode.DocumentRangeFormattingEditProvider
{
  constructor(private readonly bridge: AnalyzerBridge) {}

  provideDocumentFormattingEdits(document: vscode.TextDocument): vscode.TextEdit[] {
    const options = readOptions();
    if (!options) return [];
    const uri = document.uri.toString();
    this.bridge.updateFile(uri, document.getText());
    return toEdits(this.bridge.formatting(uri, options), document);
  }

  provideDocumentRangeFormattingEdits(
    document: vscode.TextDocument,
    range: vscode.Range,
  ): vscode.TextEdit[] {
    const options = readOptions();
    if (!options) return [];
    const uri = document.uri.toString();
    this.bridge.updateFile(uri, document.getText());
    // A selection ending at column 0 stops before that line, so folding
    // it into the range would format one line too many.
    const endLine =
      range.end.character === 0 && range.end.line > range.start.line
        ? range.end.line - 1
        : range.end.line;
    return toEdits(
      this.bridge.formattingRange(uri, range.start.line, endLine, options),
      document,
    );
  }
}

function readOptions(): FormatOptions | null {
  const config = vscode.workspace.getConfiguration('dotenv');
  if (!config.get<boolean>('format.enabled', true)) return null;
  return {
    max_blank_lines: Math.max(0, Math.floor(config.get<number>('format.maxBlankLines', 1))),
    insert_final_newline: config.get<boolean>('format.insertFinalNewline', true),
  };
}

function toEdits(
  edit: AnalyzerTextEdit | null,
  document: vscode.TextDocument,
): vscode.TextEdit[] {
  if (!edit) return [];
  const range = new vscode.Range(
    new vscode.Position(edit.start.line, edit.start.col),
    new vscode.Position(edit.end.line, edit.end.col),
  );
  // The analyzer always emits LF; match whatever the document uses.
  const text =
    document.eol === vscode.EndOfLine.CRLF
      ? edit.new_text.replace(/\r?\n/g, '\r\n')
      : edit.new_text;
  return [vscode.TextEdit.replace(range, text)];
}
