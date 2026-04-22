import * as vscode from 'vscode';
import type { AnalyzerBridge } from '../analyzer';

export class ProtoFormattingProvider implements vscode.DocumentFormattingEditProvider {
  constructor(private readonly bridge: AnalyzerBridge) {}

  provideDocumentFormattingEdits(document: vscode.TextDocument): vscode.TextEdit[] {
    const edit = this.bridge.formatting(document.uri.toString());
    if (!edit) return [];
    const range = new vscode.Range(
      new vscode.Position(edit.start.line, edit.start.col),
      new vscode.Position(edit.end.line, edit.end.col),
    );
    return [vscode.TextEdit.replace(range, edit.new_text)];
  }
}
