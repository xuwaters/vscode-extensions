import * as vscode from 'vscode';
import type { AnalyzerBridge } from '../analyzer';

export class ProtoHoverProvider implements vscode.HoverProvider {
  constructor(private readonly bridge: AnalyzerBridge) {}

  provideHover(document: vscode.TextDocument, position: vscode.Position): vscode.Hover | null {
    const h = this.bridge.hover(document.uri.toString(), position.line, position.character);
    if (!h) return null;
    const range = new vscode.Range(
      new vscode.Position(h.start.line, h.start.col),
      new vscode.Position(h.end.line, h.end.col),
    );
    const md = new vscode.MarkdownString(h.markdown);
    md.isTrusted = false;
    return new vscode.Hover(md, range);
  }
}
