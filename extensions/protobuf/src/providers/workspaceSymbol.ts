import * as vscode from 'vscode';
import type { AnalyzerBridge } from '../analyzer';

const WORKSPACE_SYMBOL_KIND: Record<string, vscode.SymbolKind> = {
  Message: vscode.SymbolKind.Class,
  Enum: vscode.SymbolKind.Enum,
  EnumValue: vscode.SymbolKind.EnumMember,
  Field: vscode.SymbolKind.Field,
  Oneof: vscode.SymbolKind.Interface,
  Service: vscode.SymbolKind.Module,
  Rpc: vscode.SymbolKind.Method,
};

export class ProtoWorkspaceSymbolProvider implements vscode.WorkspaceSymbolProvider {
  constructor(private readonly bridge: AnalyzerBridge) {}

  provideWorkspaceSymbols(query: string): vscode.SymbolInformation[] {
    const items = this.bridge.workspaceSymbols(query);
    const lower = query.toLowerCase();
    return items
      .filter((item) => item.name.toLowerCase().includes(lower) || item.fqn.toLowerCase().includes(lower))
      .map((item) => {
        const kind = WORKSPACE_SYMBOL_KIND[item.kind] ?? vscode.SymbolKind.Object;
        // We only have byte offsets in workspace symbols (spans.ByteSpan is
        // serialized as {start,end}). A zero-length zero-position range is
        // acceptable — VSCode uses the location for navigation only.
        const uri = safeParseUri(item.file);
        const range = new vscode.Range(0, 0, 0, 0);
        return new vscode.SymbolInformation(item.name, kind, item.fqn, new vscode.Location(uri, range));
      });
  }
}

function safeParseUri(uri: string): vscode.Uri {
  try {
    return vscode.Uri.parse(uri);
  } catch {
    return vscode.Uri.file(uri);
  }
}
