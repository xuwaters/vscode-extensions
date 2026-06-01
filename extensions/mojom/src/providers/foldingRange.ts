import * as vscode from 'vscode';
import type { AnalyzerBridge } from '../analyzer';

export class MojomFoldingRangeProvider implements vscode.FoldingRangeProvider {
  constructor(private readonly bridge: AnalyzerBridge) {}

  provideFoldingRanges(document: vscode.TextDocument): vscode.ProviderResult<vscode.FoldingRange[]> {
    this.bridge.updateFile(document.uri.toString(), document.getText());
    return this.bridge
      .foldingRanges(document.uri.toString())
      .filter((r) => r.end_line > r.start_line)
      .map((r) => new vscode.FoldingRange(r.start_line, r.end_line, vscode.FoldingRangeKind.Region));
  }
}
