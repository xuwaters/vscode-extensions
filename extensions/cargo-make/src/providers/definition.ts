import * as vscode from 'vscode';
import type { AnalyzerBridge } from '../analyzer';

export class CargoMakeDefinitionProvider implements vscode.DefinitionProvider {
  constructor(private readonly bridge: AnalyzerBridge) {}

  provideDefinition(
    document: vscode.TextDocument,
    position: vscode.Position,
  ): vscode.Definition | undefined {
    const uri = document.uri.toString();
    this.bridge.updateFile(uri, document.getText());
    const loc = this.bridge.definition(uri, position.line, position.character);
    if (!loc) return undefined;
    // References resolve within the same document.
    return new vscode.Location(
      document.uri,
      new vscode.Range(
        new vscode.Position(loc.start.line, loc.start.col),
        new vscode.Position(loc.end.line, loc.end.col),
      ),
    );
  }
}
