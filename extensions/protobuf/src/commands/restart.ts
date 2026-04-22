import * as vscode from 'vscode';
import type { AnalyzerBridge } from '../analyzer';
import { preloadWorkspace } from '../workspaceBootstrap';
import { resolveIncludePaths } from '../includePaths';
import { isAnalyzerLanguage, refreshDiagnostics } from '../diagnostics';

/**
 * Re-seed the analyzer with current include paths and all open/discoverable
 * `.proto` files. Useful after editing `proto3.includePaths` or when the
 * analyzer has gotten out of sync.
 */
export async function restart(
  bridge: AnalyzerBridge,
  diagCollection: vscode.DiagnosticCollection,
): Promise<void> {
  bridge.setIncludePaths(resolveIncludePaths());
  await preloadWorkspace(bridge);
  for (const doc of vscode.workspace.textDocuments) {
    if (isAnalyzerLanguage(doc.languageId)) {
      refreshDiagnostics(bridge, doc, diagCollection);
    }
  }
}
