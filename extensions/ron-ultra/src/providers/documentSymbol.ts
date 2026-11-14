import * as vscode from 'vscode';
import type { AnalyzerBridge } from '../analyzer';
import type { AnalyzerDocumentSymbol } from '../types';

export class RonDocumentSymbolProvider implements vscode.DocumentSymbolProvider {
  constructor(private readonly bridge: AnalyzerBridge) {}

  provideDocumentSymbols(document: vscode.TextDocument): vscode.DocumentSymbol[] {
    const uri = document.uri.toString();
    this.bridge.updateFile(uri, document.getText());
    return this.bridge.documentSymbols(uri).map(toVsCodeSymbol);
  }
}

function toVsCodeSymbol(s: AnalyzerDocumentSymbol): vscode.DocumentSymbol {
  const range = new vscode.Range(
    s.range_start.line,
    s.range_start.col,
    s.range_end.line,
    s.range_end.col,
  );
  const selection = new vscode.Range(
    s.selection_start.line,
    s.selection_start.col,
    s.selection_end.line,
    s.selection_end.col,
  );
  const kind =
    vscode.SymbolKind[s.kind as keyof typeof vscode.SymbolKind] ?? vscode.SymbolKind.Field;
  const symbol = new vscode.DocumentSymbol(s.name, s.detail, kind, range, selection);
  symbol.children = s.children.map(toVsCodeSymbol);
  return symbol;
}
