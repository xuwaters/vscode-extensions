import * as vscode from 'vscode';
import type { AnalyzerBridge } from '../analyzer';

export class ProtoRenameProvider implements vscode.RenameProvider {
  constructor(private readonly bridge: AnalyzerBridge) {}

  prepareRename(
    document: vscode.TextDocument,
    position: vscode.Position,
  ): vscode.Range | null {
    const r = this.bridge.prepareRename(document.uri.toString(), position.line, position.character);
    if (!r) return null;
    return new vscode.Range(
      new vscode.Position(r.start.line, r.start.col),
      new vscode.Position(r.end.line, r.end.col),
    );
  }

  provideRenameEdits(
    document: vscode.TextDocument,
    position: vscode.Position,
    newName: string,
  ): vscode.WorkspaceEdit | null {
    const edits = this.bridge.rename(
      document.uri.toString(),
      position.line,
      position.character,
      newName,
    );
    if (!edits) return null;
    const out = new vscode.WorkspaceEdit();
    for (const e of edits) {
      const uri = parseUri(e.file);
      const range = new vscode.Range(
        new vscode.Position(e.start.line, e.start.col),
        new vscode.Position(e.end.line, e.end.col),
      );
      out.replace(uri, range, e.new_text);
    }
    return out;
  }
}

function parseUri(s: string): vscode.Uri {
  try {
    return vscode.Uri.parse(s);
  } catch {
    return vscode.Uri.file(s);
  }
}
