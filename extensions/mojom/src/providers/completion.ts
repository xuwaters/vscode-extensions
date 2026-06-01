import * as vscode from 'vscode';
import type { AnalyzerBridge } from '../analyzer';

export class MojomCompletionProvider implements vscode.CompletionItemProvider {
  constructor(private readonly bridge: AnalyzerBridge) {}

  provideCompletionItems(
    document: vscode.TextDocument,
    position: vscode.Position,
  ): vscode.ProviderResult<vscode.CompletionItem[]> {
    this.bridge.updateFile(document.uri.toString(), document.getText());
    const items = this.bridge.completion(
      document.uri.toString(),
      position.line,
      position.character,
    );
    return items.map((i) => {
      const c = new vscode.CompletionItem(i.label, mapKind(i.kind));
      c.insertText = i.insert_text;
      c.detail = i.detail;
      return c;
    });
  }
}

function mapKind(kind: string): vscode.CompletionItemKind {
  switch (kind) {
    case 'keyword': return vscode.CompletionItemKind.Keyword;
    case 'struct': return vscode.CompletionItemKind.Struct;
    case 'enum': return vscode.CompletionItemKind.Enum;
    case 'interface': return vscode.CompletionItemKind.Interface;
    case 'type': return vscode.CompletionItemKind.TypeParameter;
    default: return vscode.CompletionItemKind.Reference;
  }
}
