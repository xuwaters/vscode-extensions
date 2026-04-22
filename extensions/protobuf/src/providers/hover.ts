import * as vscode from 'vscode';
import type { AnalyzerBridge } from '../analyzer';

export class ProtoHoverProvider implements vscode.HoverProvider {
  constructor(private readonly bridge: AnalyzerBridge) {}

  provideHover(document: vscode.TextDocument, position: vscode.Position): vscode.Hover | null {
    const uri = document.uri.toString();
    const h =
      document.languageId === 'textproto'
        ? this.bridge.textprotoHover(uri, position.line, position.character)
        : this.bridge.hover(uri, position.line, position.character);
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
