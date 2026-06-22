import * as vscode from 'vscode';
import type { AnalyzerBridge } from '../analyzer';

export class CargoMakeHoverProvider implements vscode.HoverProvider {
  constructor(private readonly bridge: AnalyzerBridge) {}

  provideHover(
    document: vscode.TextDocument,
    position: vscode.Position,
  ): vscode.Hover | undefined {
    const uri = document.uri.toString();
    this.bridge.updateFile(uri, document.getText());
    const h = this.bridge.hover(uri, position.line, position.character);
    if (!h) return undefined;
    const range = new vscode.Range(
      new vscode.Position(h.start.line, h.start.col),
      new vscode.Position(h.end.line, h.end.col),
    );
    return new vscode.Hover(new vscode.MarkdownString(h.contents), range);
  }
}
