import * as vscode from 'vscode';
import type { AnalyzerBridge } from '../analyzer';

export class ProtoReferenceProvider implements vscode.ReferenceProvider {
  constructor(private readonly bridge: AnalyzerBridge) {}

  provideReferences(
    document: vscode.TextDocument,
    position: vscode.Position,
    context: vscode.ReferenceContext,
  ): vscode.Location[] {
    const raw = this.bridge.references(
      document.uri.toString(),
      position.line,
      position.character,
      context.includeDeclaration,
    );
    return raw.map((r) => {
      const uri = parseUri(r.file);
      const range = new vscode.Range(
        new vscode.Position(r.start.line, r.start.col),
        new vscode.Position(r.end.line, r.end.col),
      );
      return new vscode.Location(uri, range);
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
