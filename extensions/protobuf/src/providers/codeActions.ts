import * as vscode from 'vscode';
import type { AnalyzerBridge } from '../analyzer';

export class ProtoCodeActionProvider implements vscode.CodeActionProvider {
  static readonly providedKinds = [
    vscode.CodeActionKind.QuickFix,
    vscode.CodeActionKind.SourceOrganizeImports,
  ];

  constructor(private readonly bridge: AnalyzerBridge) {}

  provideCodeActions(
    document: vscode.TextDocument,
    range: vscode.Range | vscode.Selection,
    context: vscode.CodeActionContext,
  ): vscode.CodeAction[] {
    const diagCodes = context.diagnostics
      .map((d) => (typeof d.code === 'object' && d.code !== null ? String(d.code.value) : String(d.code ?? '')))
      .filter((c) => c.length > 0);
    const raw = this.bridge.codeActions(
      document.uri.toString(),
      range.start.line,
      range.start.character,
      diagCodes,
    );
    return raw.map((a) => {
      const kind = a.kind.startsWith('source.organize')
        ? vscode.CodeActionKind.SourceOrganizeImports
        : vscode.CodeActionKind.QuickFix;
      const action = new vscode.CodeAction(a.title, kind);
      const edit = new vscode.WorkspaceEdit();
      for (const e of a.edits) {
        const uri = parseUri(e.file);
        const r = new vscode.Range(
          new vscode.Position(e.start.line, e.start.col),
          new vscode.Position(e.end.line, e.end.col),
        );
        edit.replace(uri, r, e.new_text);
      }
      action.edit = edit;
      return action;
    });
  }
}

function parseUri(s: string): vscode.Uri {
  try {
    return vscode.Uri.parse(s);
  } catch {
    return vscode.Uri.file(s);
  }
}
