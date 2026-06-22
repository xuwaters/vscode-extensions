import * as vscode from 'vscode';
import type { AnalyzerBridge } from '../analyzer';

export class CargoMakeFoldingRangeProvider implements vscode.FoldingRangeProvider {
  constructor(private readonly bridge: AnalyzerBridge) {}

  provideFoldingRanges(document: vscode.TextDocument): vscode.FoldingRange[] {
    const uri = document.uri.toString();
    this.bridge.updateFile(uri, document.getText());
    return this.bridge
      .foldingRanges(uri)
      .map((r) => new vscode.FoldingRange(r.start_line, r.end_line, vscode.FoldingRangeKind.Region));
  }
}
