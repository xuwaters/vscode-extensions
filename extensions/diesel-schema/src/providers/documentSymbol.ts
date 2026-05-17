import * as vscode from 'vscode';
import type { AnalyzerBridge } from '../analyzer';
import type { AnalyzerDocumentSymbol, AnalyzerSymbolKind } from '../types';
import { isDieselSchema } from '../util';

const KIND_MAP: Record<AnalyzerSymbolKind, vscode.SymbolKind> = {
  Table: vscode.SymbolKind.Class,
  Column: vscode.SymbolKind.Field,
  Joinable: vscode.SymbolKind.Method,
  AllowGroup: vscode.SymbolKind.Namespace,
};

export class DieselDocumentSymbolProvider implements vscode.DocumentSymbolProvider {
  constructor(private readonly bridge: AnalyzerBridge) {}

  provideDocumentSymbols(document: vscode.TextDocument): vscode.DocumentSymbol[] {
    if (!isDieselSchema(document)) return [];
    const uri = document.uri.toString();
    this.bridge.updateFile(uri, document.getText());
    return this.bridge.documentSymbols(uri).map(toVsCodeSymbol);
  }
}

function toVsCodeSymbol(s: AnalyzerDocumentSymbol): vscode.DocumentSymbol {
  const range = new vscode.Range(
    new vscode.Position(s.range_start.line, s.range_start.col),
    new vscode.Position(s.range_end.line, s.range_end.col),
  );
  const selection = new vscode.Range(
    new vscode.Position(s.selection_start.line, s.selection_start.col),
    new vscode.Position(s.selection_end.line, s.selection_end.col),
  );
  const sym = new vscode.DocumentSymbol(s.name, s.detail, KIND_MAP[s.kind], range, selection);
  sym.children = s.children.map(toVsCodeSymbol);
  return sym;
}
