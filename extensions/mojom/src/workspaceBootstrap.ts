import * as vscode from 'vscode';
import type { AnalyzerBridge } from './analyzer';

/**
 * Scan the workspace for `.mojom` files and seed the analyzer with their
 * contents so cross-file features (imports, workspace symbols, go-to-def)
 * work without requiring the user to open every file first. Uses
 * `preloadFile` to avoid clobbering the user's open buffer if it has already
 * been fed through `updateFile`.
 */
export async function preloadWorkspace(bridge: AnalyzerBridge): Promise<void> {
  if (!bridge.ready) return;
  const exclude = '{**/node_modules/**,**/target/**,**/dist/**,**/build/**,**/out/**}';
  const files = await vscode.workspace.findFiles('**/*.mojom', exclude, 5000);
  for (const uri of files) {
    try {
      const bytes = await vscode.workspace.fs.readFile(uri);
      const text = new TextDecoder('utf-8').decode(bytes);
      bridge.preloadFile(uri.toString(), text);
    } catch {
      // ignore — file may have been removed between find and read
    }
  }
}
