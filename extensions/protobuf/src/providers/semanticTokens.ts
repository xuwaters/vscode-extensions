import * as vscode from 'vscode';
import type { AnalyzerBridge } from '../analyzer';

const TOKEN_TYPES = ['type', 'enum', 'enumMember', 'property', 'function', 'namespace'];

export const SEMANTIC_TOKENS_LEGEND = new vscode.SemanticTokensLegend(TOKEN_TYPES, []);

const KIND_TO_INDEX: Record<string, number> = {
  Type: 0,
  Enum: 1,
  EnumMember: 2,
  Property: 3,
  Function: 4,
  Namespace: 5,
};

export class ProtoSemanticTokensProvider implements vscode.DocumentSemanticTokensProvider {
  constructor(private readonly bridge: AnalyzerBridge) {}

  provideDocumentSemanticTokens(document: vscode.TextDocument): vscode.SemanticTokens {
    const builder = new vscode.SemanticTokensBuilder(SEMANTIC_TOKENS_LEGEND);
    const tokens = this.bridge.semanticTokens(document.uri.toString());
    for (const t of tokens) {
      const idx = KIND_TO_INDEX[t.token_type];
      if (idx === undefined) continue;
      builder.push(t.line, t.col, t.length, idx, 0);
    }
    return builder.build();
  }
}
