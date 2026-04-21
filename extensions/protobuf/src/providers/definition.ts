import * as vscode from 'vscode';
import type { AnalyzerBridge } from '../analyzer';

export class ProtoDefinitionProvider implements vscode.DefinitionProvider {
  constructor(private readonly bridge: AnalyzerBridge) {}

  provideDefinition(
    document: vscode.TextDocument,
    position: vscode.Position,
  ): vscode.Definition | null {
    const loc = this.bridge.definition(document.uri.toString(), position.line, position.character);
    if (!loc) return null;
    const uri = parseUri(loc.file);
    const range = new vscode.Range(
      new vscode.Position(loc.start.line, loc.start.col),
      new vscode.Position(loc.end.line, loc.end.col),
    );
    return new vscode.Location(uri, range);
  }
}

function parseUri(s: string): vscode.Uri {
  try {
    return vscode.Uri.parse(s);
  } catch {
    return vscode.Uri.file(s);
  }
}
