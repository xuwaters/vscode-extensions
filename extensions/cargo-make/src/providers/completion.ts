import * as vscode from 'vscode';
import type { AnalyzerBridge } from '../analyzer';
import type { AnalyzerCompletionItem, AnalyzerCompletionKind } from '../types';

const KIND_MAP: Record<AnalyzerCompletionKind, vscode.CompletionItemKind> = {
  Field: vscode.CompletionItemKind.Field,
  EnumValue: vscode.CompletionItemKind.EnumMember,
  Task: vscode.CompletionItemKind.Method,
  Section: vscode.CompletionItemKind.Module,
};

export class CargoMakeCompletionProvider implements vscode.CompletionItemProvider {
  constructor(private readonly bridge: AnalyzerBridge) {}

  provideCompletionItems(
    document: vscode.TextDocument,
    position: vscode.Position,
  ): vscode.CompletionItem[] {
    const uri = document.uri.toString();
    this.bridge.updateFile(uri, document.getText());
    const items = this.bridge.complete(uri, position.line, position.character);
    return items.map((item) => toVsCodeItem(item, position));
  }
}

function toVsCodeItem(
  item: AnalyzerCompletionItem,
  position: vscode.Position,
): vscode.CompletionItem {
  const kind = KIND_MAP[item.kind] ?? vscode.CompletionItemKind.Text;
  const ci = new vscode.CompletionItem(item.label, kind);
  if (item.detail) {
    ci.detail = item.detail;
  }
  ci.insertText = item.insert_text;
  // Pin the replacement range to what the analyzer reports so a prefix
  // already typed (including leading `@` for runner names) is overwritten
  // rather than duplicated.
  const start = position.translate(0, -item.replace_length);
  ci.range = new vscode.Range(start, position);
  return ci;
}
