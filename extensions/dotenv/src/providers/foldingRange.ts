import * as vscode from 'vscode';
import type { AnalyzerBridge } from '../analyzer';

export class DotenvFoldingRangeProvider implements vscode.FoldingRangeProvider {
  constructor(private readonly bridge: AnalyzerBridge) {}

  provideFoldingRanges(document: vscode.TextDocument): vscode.FoldingRange[] {
    const uri = document.uri.toString();
    this.bridge.updateFile(uri, document.getText());
    const raw = this.bridge.foldingRanges(uri);
    return raw.map((r) => {
      const kind =
        r.kind === 'Comment' ? vscode.FoldingRangeKind.Comment : vscode.FoldingRangeKind.Region;
      return new vscode.FoldingRange(r.start_line, r.end_line, kind);
    });
  }
}
