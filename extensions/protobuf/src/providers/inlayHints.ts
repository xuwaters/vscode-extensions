import * as vscode from 'vscode';
import type { AnalyzerBridge } from '../analyzer';

export class ProtoInlayHintsProvider implements vscode.InlayHintsProvider {
  constructor(private readonly bridge: AnalyzerBridge) {}

  provideInlayHints(document: vscode.TextDocument): vscode.InlayHint[] {
    return this.bridge.inlayHints(document.uri.toString()).map((h) => {
      const hint = new vscode.InlayHint(
        new vscode.Position(h.line, h.col),
        h.label,
        vscode.InlayHintKind.Type,
      );
      hint.paddingLeft = true;
      return hint;
    });
  }
}
