import * as vscode from 'vscode';
import { AnalyzerBridge } from './analyzer';
import { refreshDiagnostics } from './diagnostics';
import { resolveIncludePaths } from './includePaths';
import { preloadWorkspace } from './workspaceBootstrap';
import { ProtoDocumentSymbolProvider } from './providers/documentSymbol';
import { ProtoWorkspaceSymbolProvider } from './providers/workspaceSymbol';
import { ProtoDefinitionProvider } from './providers/definition';
import { ProtoHoverProvider } from './providers/hover';
import { ProtoCompletionProvider } from './providers/completion';
import { ProtoFoldingRangeProvider } from './providers/foldingRange';
import { restart } from './commands/restart';
import { showSymbolTree } from './commands/showSymbolTree';

const PROTO3_SELECTOR: vscode.DocumentSelector = { scheme: 'file', language: 'proto3' };

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  const bridge = AnalyzerBridge.load(context.extensionPath);

  const diagCollection = vscode.languages.createDiagnosticCollection('proto3');
  context.subscriptions.push(diagCollection);

  bridge.setIncludePaths(resolveIncludePaths());
  await preloadWorkspace(bridge);

  context.subscriptions.push(
    vscode.languages.registerDocumentSymbolProvider(PROTO3_SELECTOR, new ProtoDocumentSymbolProvider(bridge)),
    vscode.languages.registerWorkspaceSymbolProvider(new ProtoWorkspaceSymbolProvider(bridge)),
    vscode.languages.registerDefinitionProvider(PROTO3_SELECTOR, new ProtoDefinitionProvider(bridge)),
    vscode.languages.registerHoverProvider(PROTO3_SELECTOR, new ProtoHoverProvider(bridge)),
    vscode.languages.registerCompletionItemProvider(
      PROTO3_SELECTOR,
      new ProtoCompletionProvider(bridge),
      '.',
      '/',
    ),
    vscode.languages.registerFoldingRangeProvider(PROTO3_SELECTOR, new ProtoFoldingRangeProvider(bridge)),
  );

  for (const doc of vscode.workspace.textDocuments) {
    if (doc.languageId === 'proto3') {
      refreshDiagnostics(bridge, doc, diagCollection);
    }
  }

  let debounce: ReturnType<typeof setTimeout> | undefined;
  context.subscriptions.push(
    vscode.workspace.onDidOpenTextDocument((doc) => {
      if (doc.languageId === 'proto3') refreshDiagnostics(bridge, doc, diagCollection);
    }),
    vscode.workspace.onDidChangeTextDocument((e) => {
      if (e.document.languageId !== 'proto3') return;
      const onType = vscode.workspace.getConfiguration('proto3').get<boolean>('diagnostics.onType', true);
      if (!onType) return;
      if (debounce) clearTimeout(debounce);
      debounce = setTimeout(() => refreshDiagnostics(bridge, e.document, diagCollection), 150);
    }),
    vscode.workspace.onDidSaveTextDocument((doc) => {
      if (doc.languageId === 'proto3') refreshDiagnostics(bridge, doc, diagCollection);
    }),
    vscode.workspace.onDidCloseTextDocument((doc) => {
      diagCollection.delete(doc.uri);
    }),
    vscode.workspace.onDidDeleteFiles((e) => {
      for (const uri of e.files) {
        bridge.removeFile(uri.toString());
        diagCollection.delete(uri);
      }
    }),
    vscode.workspace.onDidChangeConfiguration((e) => {
      if (e.affectsConfiguration('proto3.includePaths')) {
        bridge.setIncludePaths(resolveIncludePaths());
        for (const doc of vscode.workspace.textDocuments) {
          if (doc.languageId === 'proto3') refreshDiagnostics(bridge, doc, diagCollection);
        }
      }
    }),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand('proto3.restart', () => restart(bridge, diagCollection)),
    vscode.commands.registerCommand('proto3.showSymbolTree', () => showSymbolTree(bridge)),
  );
}

export function deactivate(): void {}
