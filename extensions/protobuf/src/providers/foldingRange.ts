import * as vscode from 'vscode';
import type { AnalyzerBridge } from '../analyzer';

export class ProtoFoldingRangeProvider implements vscode.FoldingRangeProvider {
  constructor(private readonly bridge: AnalyzerBridge) {}

  provideFoldingRanges(document: vscode.TextDocument): vscode.FoldingRange[] {
    const uri = document.uri.toString();
    const raw =
      document.languageId === 'textproto'
        ? this.bridge.textprotoFoldingRanges(uri)
        : this.bridge.foldingRanges(uri);
    return raw
      .filter((r) => r.end_line > r.start_line)
      .map((r) => {
        const kind =
          r.kind === 'Comment' ? vscode.FoldingRangeKind.Comment : vscode.FoldingRangeKind.Region;
        return new vscode.FoldingRange(r.start_line, r.end_line, kind);
      });
  }
}
