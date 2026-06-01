import * as vscode from 'vscode';
import type { AnalyzerBridge } from '../analyzer';

export class MojomDefinitionProvider implements vscode.DefinitionProvider {
  constructor(private readonly bridge: AnalyzerBridge) {}

  provideDefinition(
    document: vscode.TextDocument,
    position: vscode.Position,
  ): vscode.ProviderResult<vscode.Definition> {
    this.bridge.updateFile(document.uri.toString(), document.getText());
    const loc = this.bridge.definition(
      document.uri.toString(),
      position.line,
      position.character,
    );
    if (!loc) return undefined;
    const target = vscode.Uri.parse(loc.file);
    return new vscode.Location(
      target,
      new vscode.Range(
        new vscode.Position(loc.start.line, loc.start.col),
        new vscode.Position(loc.end.line, loc.end.col),
      ),
    );
  }
}
