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
import { ProtoReferenceProvider } from './providers/references';
import { ProtoRenameProvider } from './providers/rename';
import { ProtoFormattingProvider } from './providers/formatting';
import { ProtoInlayHintsProvider } from './providers/inlayHints';
import {
  ProtoSemanticTokensProvider,
  SEMANTIC_TOKENS_LEGEND,
} from './providers/semanticTokens';
import { ProtoCodeActionProvider } from './providers/codeActions';
import { restart } from './commands/restart';
import { showSymbolTree } from './commands/showSymbolTree';

const PROTO3_SELECTOR: vscode.DocumentSelector = { scheme: 'file', language: 'proto3' };

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  const bridge = AnalyzerBridge.load(context.extensionPath);

  const diagCollection = vscode.languages.createDiagnosticCollection('proto3');
  context.subscriptions.push(diagCollection);

  bridge.setIncludePaths(resolveIncludePaths());
  bridge.setStyleEnabled(
    vscode.workspace.getConfiguration('proto3').get<string>('diagnostics.style', 'off') === 'on',
  );
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
    vscode.languages.registerReferenceProvider(PROTO3_SELECTOR, new ProtoReferenceProvider(bridge)),
    vscode.languages.registerRenameProvider(PROTO3_SELECTOR, new ProtoRenameProvider(bridge)),
    vscode.languages.registerDocumentFormattingEditProvider(
      PROTO3_SELECTOR,
      new ProtoFormattingProvider(bridge),
    ),
    vscode.languages.registerInlayHintsProvider(PROTO3_SELECTOR, new ProtoInlayHintsProvider(bridge)),
    vscode.languages.registerDocumentSemanticTokensProvider(
      PROTO3_SELECTOR,
      new ProtoSemanticTokensProvider(bridge),
      SEMANTIC_TOKENS_LEGEND,
    ),
    vscode.languages.registerCodeActionsProvider(
      PROTO3_SELECTOR,
      new ProtoCodeActionProvider(bridge),
      { providedCodeActionKinds: ProtoCodeActionProvider.providedKinds },
    ),
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
      }
      if (e.affectsConfiguration('proto3.diagnostics.style')) {
        bridge.setStyleEnabled(
          vscode.workspace.getConfiguration('proto3').get<string>('diagnostics.style', 'off') === 'on',
        );
      }
      if (
        e.affectsConfiguration('proto3.includePaths') ||
        e.affectsConfiguration('proto3.diagnostics.style')
      ) {
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
