import * as vscode from 'vscode';
import type { AnalyzerBridge } from '../analyzer';
import { isDieselSchema } from '../util';

export class DieselFoldingRangeProvider implements vscode.FoldingRangeProvider {
  constructor(private readonly bridge: AnalyzerBridge) {}

  provideFoldingRanges(document: vscode.TextDocument): vscode.FoldingRange[] {
    if (!isDieselSchema(document)) return [];
    const uri = document.uri.toString();
    this.bridge.updateFile(uri, document.getText());
    return this.bridge
      .foldingRanges(uri)
      .map((r) => new vscode.FoldingRange(r.start_line, r.end_line, vscode.FoldingRangeKind.Region));
  }
}
