import * as vscode from 'vscode';
import type { AnalyzerBridge } from '../analyzer';
import type { AnalyzerDocumentSymbol } from '../types';

const SYMBOL_KIND_MAP: Record<string, vscode.SymbolKind> = {
  Package: vscode.SymbolKind.Package,
  Message: vscode.SymbolKind.Class,
  Enum: vscode.SymbolKind.Enum,
  EnumMember: vscode.SymbolKind.EnumMember,
  Field: vscode.SymbolKind.Field,
  Oneof: vscode.SymbolKind.Interface,
  Service: vscode.SymbolKind.Module,
  Method: vscode.SymbolKind.Method,
};

export class ProtoDocumentSymbolProvider implements vscode.DocumentSymbolProvider {
  constructor(private readonly bridge: AnalyzerBridge) {}

  provideDocumentSymbols(document: vscode.TextDocument): vscode.DocumentSymbol[] {
    const uri = document.uri.toString();
    let raw;
    if (document.languageId === 'textproto') {
      this.bridge.updateTextprotoFile(uri, document.getText());
      raw = this.bridge.textprotoDocumentSymbols(uri);
    } else {
      this.bridge.updateFile(uri, document.getText());
      raw = this.bridge.documentSymbols(uri);
    }
    return raw.map(toVsCodeSymbol);
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
  const kind = SYMBOL_KIND_MAP[s.kind] ?? vscode.SymbolKind.Object;
  const sym = new vscode.DocumentSymbol(s.name, s.detail, kind, range, selection);
  sym.children = s.children.map(toVsCodeSymbol);
  return sym;
}
