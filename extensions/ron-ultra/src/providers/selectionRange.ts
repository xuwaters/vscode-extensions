import * as vscode from 'vscode';
import type { AnalyzerBridge } from '../analyzer';

export class RonSelectionRangeProvider implements vscode.SelectionRangeProvider {
  constructor(private readonly bridge: AnalyzerBridge) {}

  provideSelectionRanges(
    document: vscode.TextDocument,
    positions: vscode.Position[],
  ): vscode.SelectionRange[] {
    const uri = document.uri.toString();
    this.bridge.updateFile(uri, document.getText());
    const chains = this.bridge.selectionRanges(
      uri,
      positions.map((p) => ({ line: p.line, col: p.character })),
    );
    return positions.map((position, i) => {
      // Build outermost-in so each range can point at its parent.
      let parent: vscode.SelectionRange | undefined;
      for (const r of [...(chains[i] ?? [])].reverse()) {
        parent = new vscode.SelectionRange(
          new vscode.Range(r.start.line, r.start.col, r.end.line, r.end.col),
          parent,
        );
      }
      return parent ?? new vscode.SelectionRange(new vscode.Range(position, position));
    });
  }
}
