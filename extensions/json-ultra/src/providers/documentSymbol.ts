import * as vscode from 'vscode';
import type { AnalyzerBridge } from '../analyzer.js';
import type { AnalyzerDocumentSymbol, SymbolValueKind } from '../types.js';

const KIND_MAP: Record<SymbolValueKind, vscode.SymbolKind> = {
  object: vscode.SymbolKind.Object,
  array: vscode.SymbolKind.Array,
  string: vscode.SymbolKind.String,
  number: vscode.SymbolKind.Number,
  boolean: vscode.SymbolKind.Boolean,
  null: vscode.SymbolKind.Null,
};

function convert(symbol: AnalyzerDocumentSymbol): vscode.DocumentSymbol {
  const range = new vscode.Range(
    new vscode.Position(symbol.range_start.line, symbol.range_start.col),
    new vscode.Position(symbol.range_end.line, symbol.range_end.col),
  );
  const selection = new vscode.Range(
    new vscode.Position(symbol.selection_start.line, symbol.selection_start.col),
    new vscode.Position(symbol.selection_end.line, symbol.selection_end.col),
  );
  const out = new vscode.DocumentSymbol(
    symbol.name.length > 0 ? symbol.name : '(empty)',
    symbol.detail,
    KIND_MAP[symbol.kind] ?? vscode.SymbolKind.Field,
    range,
    selection,
  );
  out.children = symbol.children.map(convert);
  return out;
}

export class JsonDocumentSymbolProvider implements vscode.DocumentSymbolProvider {
  constructor(private readonly bridge: AnalyzerBridge) {}

  provideDocumentSymbols(document: vscode.TextDocument): vscode.DocumentSymbol[] {
    const uri = document.uri.toString();
    this.bridge.updateFile(uri, document.getText(), document.languageId);
    return this.bridge.documentSymbols(uri).map(convert);
  }
}
