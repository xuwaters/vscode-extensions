import * as vscode from 'vscode';
import type { AnalyzerBridge } from './analyzer';

/**
 * Scan the workspace for `.proto` files on activation and feed their
 * contents into the analyzer so cross-file features (diagnostics,
 * workspace symbols) work without requiring the user to open each file.
 */
export async function preloadWorkspace(bridge: AnalyzerBridge): Promise<void> {
  if (!bridge.ready) return;
  const files = await vscode.workspace.findFiles('**/*.proto', '**/node_modules/**', 2000);
  for (const uri of files) {
    try {
      const bytes = await vscode.workspace.fs.readFile(uri);
      const text = new TextDecoder('utf-8').decode(bytes);
      bridge.updateFile(uri.toString(), text);
    } catch {
      // ignore read errors — the file may have been removed between find and read
    }
  }
}
