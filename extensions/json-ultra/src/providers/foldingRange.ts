import * as vscode from 'vscode';
import type { AnalyzerBridge } from '../analyzer.js';

export class JsonFoldingRangeProvider implements vscode.FoldingRangeProvider {
  constructor(private readonly bridge: AnalyzerBridge) {}

  provideFoldingRanges(document: vscode.TextDocument): vscode.FoldingRange[] {
    const uri = document.uri.toString();
    this.bridge.updateFile(uri, document.getText(), document.languageId);
    return this.bridge.foldingRanges(uri).map(
      (r) =>
        new vscode.FoldingRange(
          r.start_line,
          r.end_line,
          r.kind === 'Comment'
            ? vscode.FoldingRangeKind.Comment
            : vscode.FoldingRangeKind.Region,
        ),
    );
  }
}
