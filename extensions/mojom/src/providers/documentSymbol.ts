import * as vscode from 'vscode';
import type { AnalyzerBridge } from '../analyzer';
import type { AnalyzerDocumentSymbol } from '../types';

export class MojomDocumentSymbolProvider implements vscode.DocumentSymbolProvider {
  constructor(private readonly bridge: AnalyzerBridge) {}

  provideDocumentSymbols(
    document: vscode.TextDocument,
  ): vscode.ProviderResult<vscode.DocumentSymbol[]> {
    this.bridge.updateFile(document.uri.toString(), document.getText());
    const symbols = this.bridge.documentSymbols(document.uri.toString());
    return symbols.map(toVscodeSymbol);
  }
}

function toVscodeSymbol(s: AnalyzerDocumentSymbol): vscode.DocumentSymbol {
  const range = new vscode.Range(
    new vscode.Position(s.range_start.line, s.range_start.col),
    new vscode.Position(s.range_end.line, s.range_end.col),
  );
  const selection = new vscode.Range(
    new vscode.Position(s.selection_start.line, s.selection_start.col),
    new vscode.Position(s.selection_end.line, s.selection_end.col),
  );
  const safeSelection = range.contains(selection) ? selection : range;
  const sym = new vscode.DocumentSymbol(s.name, s.detail, mapKind(s.kind), range, safeSelection);
  sym.children = s.children.map(toVscodeSymbol);
  return sym;
}

function mapKind(kind: string): vscode.SymbolKind {
  switch (kind) {
    case 'module': return vscode.SymbolKind.Namespace;
    case 'struct': return vscode.SymbolKind.Struct;
    case 'union': return vscode.SymbolKind.Struct;
    case 'interface': return vscode.SymbolKind.Interface;
    case 'enum': return vscode.SymbolKind.Enum;
    case 'enumMember': return vscode.SymbolKind.EnumMember;
    case 'method': return vscode.SymbolKind.Method;
    case 'field': return vscode.SymbolKind.Field;
    case 'constant': return vscode.SymbolKind.Constant;
    default: return vscode.SymbolKind.Object;
  }
}
