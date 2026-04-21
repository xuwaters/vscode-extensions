import * as vscode from 'vscode';
import type { AnalyzerBridge } from '../analyzer';

const KIND_MAP: Record<string, vscode.CompletionItemKind> = {
  Keyword: vscode.CompletionItemKind.Keyword,
  Scalar: vscode.CompletionItemKind.TypeParameter,
  Message: vscode.CompletionItemKind.Class,
  Enum: vscode.CompletionItemKind.Enum,
  EnumValue: vscode.CompletionItemKind.EnumMember,
  Field: vscode.CompletionItemKind.Field,
  Service: vscode.CompletionItemKind.Module,
  Rpc: vscode.CompletionItemKind.Method,
};

export class ProtoCompletionProvider implements vscode.CompletionItemProvider {
  constructor(private readonly bridge: AnalyzerBridge) {}

  provideCompletionItems(
    document: vscode.TextDocument,
    position: vscode.Position,
  ): vscode.CompletionItem[] {
    const raw = this.bridge.completion(document.uri.toString(), position.line, position.character);
    return raw.map((c) => {
      const item = new vscode.CompletionItem(
        c.label,
        KIND_MAP[c.kind] ?? vscode.CompletionItemKind.Text,
      );
      item.insertText = c.insert_text;
      item.detail = c.detail;
      return item;
    });
  }
}
