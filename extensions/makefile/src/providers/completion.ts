import * as vscode from 'vscode';
import type { AnalyzerBridge } from '../analyzer';
import type { AnalyzerCompletionItem, AnalyzerCompletionKind } from '../types';

const KIND_MAP: Record<AnalyzerCompletionKind, vscode.CompletionItemKind> = {
  Long: vscode.CompletionItemKind.Field,
  Short: vscode.CompletionItemKind.Field,
  ArgValue: vscode.CompletionItemKind.EnumMember,
  Subcommand: vscode.CompletionItemKind.Method,
};

export class MakefileCompletionProvider implements vscode.CompletionItemProvider {
  constructor(private readonly bridge: AnalyzerBridge) {}

  provideCompletionItems(
    document: vscode.TextDocument,
    position: vscode.Position,
  ): vscode.CompletionItem[] {
    const uri = document.uri.toString();
    this.bridge.updateFile(uri, document.getText());
    const items = this.bridge.complete(uri, position.line, position.character);
    return items.map(toVsCodeItem);
  }
}

function toVsCodeItem(item: AnalyzerCompletionItem): vscode.CompletionItem {
  const kind = KIND_MAP[item.kind] ?? vscode.CompletionItemKind.Text;
  const ci = new vscode.CompletionItem(item.label, kind);
  if (item.detail) {
    ci.detail = item.detail;
  }
  ci.insertText = item.insert_text;
  return ci;
}
