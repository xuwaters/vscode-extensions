import * as vscode from 'vscode';
import type { AnalyzerBridge } from '../analyzer';

export async function showSymbolTree(bridge: AnalyzerBridge): Promise<void> {
  const symbols = bridge.workspaceSymbols('');
  const lines = symbols
    .map((s) => `${s.kind.padEnd(10)} ${s.fqn}  (${s.file})`)
    .join('\n');
  const doc = await vscode.workspace.openTextDocument({
    language: 'plaintext',
    content: lines || '(no symbols indexed)',
  });
  await vscode.window.showTextDocument(doc, { preview: true });
}
