import * as vscode from 'vscode';
import type { AnalyzerBridge } from '../analyzer';
import type { AnalyzerDocumentSymbol } from '../types';

const SYMBOL_KIND_MAP: Record<string, vscode.SymbolKind> = {
  Target: vscode.SymbolKind.Method,
  PatternTarget: vscode.SymbolKind.Method,
  PhonyTarget: vscode.SymbolKind.Method,
  Variable: vscode.SymbolKind.Variable,
  Constant: vscode.SymbolKind.Constant,
  Include: vscode.SymbolKind.File,
  Conditional: vscode.SymbolKind.Namespace,
  Directive: vscode.SymbolKind.Key,
};

export class MakefileDocumentSymbolProvider implements vscode.DocumentSymbolProvider {
  constructor(private readonly bridge: AnalyzerBridge) {}

  provideDocumentSymbols(document: vscode.TextDocument): vscode.DocumentSymbol[] {
    const uri = document.uri.toString();
    this.bridge.updateFile(uri, document.getText());
    const raw = this.bridge.documentSymbols(uri);
    const showPhony = vscode.workspace
      .getConfiguration('makefile')
      .get<boolean>('outline.showPhonyDeclarations', false);
    const visible = showPhony ? raw : filterPhonyDeclarations(raw);
    return visible.map(toVsCodeSymbol);
  }
}

function filterPhonyDeclarations(symbols: AnalyzerDocumentSymbol[]): AnalyzerDocumentSymbol[] {
  const out: AnalyzerDocumentSymbol[] = [];
  for (const s of symbols) {
    if (s.name === '.PHONY') continue;
    out.push({ ...s, children: filterPhonyDeclarations(s.children) });
  }
  return out;
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
