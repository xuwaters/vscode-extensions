import * as vscode from 'vscode';
import type { AnalyzerBridge } from '../analyzer';

export class RonHoverProvider implements vscode.HoverProvider {
  constructor(private readonly bridge: AnalyzerBridge) {}

  provideHover(document: vscode.TextDocument, position: vscode.Position): vscode.Hover | null {
    const uri = document.uri.toString();
    this.bridge.updateFile(uri, document.getText());
    const h = this.bridge.hover(uri, position.line, position.character);
    if (!h) return null;
    const range = new vscode.Range(h.start.line, h.start.col, h.end.line, h.end.col);
    return new vscode.Hover(new vscode.MarkdownString(h.contents), range);
  }
}
