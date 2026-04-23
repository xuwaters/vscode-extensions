import * as vscode from 'vscode';
import type { AnalyzerBridge } from '../analyzer';

export class CapnpWorkspaceSymbolProvider implements vscode.WorkspaceSymbolProvider {
  constructor(private readonly bridge: AnalyzerBridge) {}

  provideWorkspaceSymbols(query: string): vscode.ProviderResult<vscode.SymbolInformation[]> {
    return this.bridge.workspaceSymbols(query).map((s) => {
      const uri = vscode.Uri.parse(s.file);
      const range = new vscode.Range(
        new vscode.Position(s.start.line, s.start.col),
        new vscode.Position(s.end.line, s.end.col),
      );
      return new vscode.SymbolInformation(s.fqn, mapKind(s.kind), '', new vscode.Location(uri, range));
    });
  }
}

function mapKind(kind: string): vscode.SymbolKind {
  switch (kind) {
    case 'struct': return vscode.SymbolKind.Struct;
    case 'enum': return vscode.SymbolKind.Enum;
    case 'enumMember': return vscode.SymbolKind.EnumMember;
    case 'interface': return vscode.SymbolKind.Interface;
    case 'method': return vscode.SymbolKind.Method;
    case 'field': return vscode.SymbolKind.Field;
    case 'constant': return vscode.SymbolKind.Constant;
    case 'annotation': return vscode.SymbolKind.Property;
    case 'namespace': return vscode.SymbolKind.Namespace;
    default: return vscode.SymbolKind.Object;
  }
}
