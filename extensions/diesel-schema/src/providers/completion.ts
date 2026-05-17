import * as vscode from 'vscode';
import type { AnalyzerBridge } from '../analyzer';
import type { AnalyzerCompletionItem } from '../types';
import { isDieselSchema } from '../util';

const KIND_MAP: Record<AnalyzerCompletionItem['kind'], vscode.CompletionItemKind> = {
  Table: vscode.CompletionItemKind.Class,
  Column: vscode.CompletionItemKind.Field,
};

export class DieselCompletionProvider implements vscode.CompletionItemProvider {
  constructor(private readonly bridge: AnalyzerBridge) {}

  provideCompletionItems(
    document: vscode.TextDocument,
    position: vscode.Position,
  ): vscode.CompletionItem[] {
    if (!isDieselSchema(document)) return [];
    const enabled = vscode.workspace
      .getConfiguration('dieselSchema')
      .get<boolean>('completion.enabled', true);
    if (!enabled) return [];
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
  const ci = new vscode.CompletionItem(item.label, KIND_MAP[item.kind]);
  if (item.detail) ci.detail = item.detail;
  ci.insertText = item.insert_text;
  const start = position.translate(0, -item.replace_length);
  ci.range = new vscode.Range(start, position);
  return ci;
}
