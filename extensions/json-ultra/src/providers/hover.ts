import * as vscode from 'vscode';
import type { AnalyzerBridge } from '../analyzer.js';

export class JsonHoverProvider implements vscode.HoverProvider {
  constructor(private readonly bridge: AnalyzerBridge) {}

  provideHover(
    document: vscode.TextDocument,
    position: vscode.Position,
  ): vscode.Hover | null {
    const uri = document.uri.toString();
    this.bridge.updateFile(uri, document.getText(), document.languageId);
    const hover = this.bridge.hover(uri, position.line, position.character);
    if (!hover) return null;
    return new vscode.Hover(
      new vscode.MarkdownString(hover.contents),
      new vscode.Range(
        new vscode.Position(hover.start.line, hover.start.col),
        new vscode.Position(hover.end.line, hover.end.col),
      ),
    );
  }
}
