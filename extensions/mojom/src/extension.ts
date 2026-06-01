import * as vscode from 'vscode';
import { AnalyzerBridge } from './analyzer';
import { isMojomDocument, refreshDiagnostics } from './diagnostics';
import { resolveIncludePaths } from './includePaths';
import { preloadWorkspace } from './workspaceBootstrap';
import { MojomDocumentSymbolProvider } from './providers/documentSymbol';
import { MojomFoldingRangeProvider } from './providers/foldingRange';
import { MojomHoverProvider } from './providers/hover';
import { MojomDefinitionProvider } from './providers/definition';
import { MojomCompletionProvider } from './providers/completion';
import { MojomWorkspaceSymbolProvider } from './providers/workspaceSymbol';

const MOJOM_SELECTOR: vscode.DocumentSelector = { scheme: 'file', language: 'mojom' };

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  const bridge = AnalyzerBridge.load(context.extensionPath);

  const diagCollection = vscode.languages.createDiagnosticCollection('mojom');
  context.subscriptions.push(diagCollection);

  bridge.setIncludePaths(resolveIncludePaths());
  await preloadWorkspace(bridge);

  context.subscriptions.push(
    vscode.languages.registerDocumentSymbolProvider(
      MOJOM_SELECTOR,
      new MojomDocumentSymbolProvider(bridge),
    ),
    vscode.languages.registerFoldingRangeProvider(
      MOJOM_SELECTOR,
      new MojomFoldingRangeProvider(bridge),
    ),
    vscode.languages.registerHoverProvider(MOJOM_SELECTOR, new MojomHoverProvider(bridge)),
    vscode.languages.registerDefinitionProvider(
      MOJOM_SELECTOR,
      new MojomDefinitionProvider(bridge),
    ),
    vscode.languages.registerCompletionItemProvider(
      MOJOM_SELECTOR,
      new MojomCompletionProvider(bridge),
      '.',
      '<',
    ),
    vscode.languages.registerWorkspaceSymbolProvider(new MojomWorkspaceSymbolProvider(bridge)),
  );

  for (const doc of vscode.workspace.textDocuments) {
    if (isMojomDocument(doc.languageId)) {
      refreshDiagnostics(bridge, doc, diagCollection);
    }
  }

  let debounce: ReturnType<typeof setTimeout> | undefined;
  const refreshAllOpen = () => {
    for (const doc of vscode.workspace.textDocuments) {
      if (isMojomDocument(doc.languageId)) refreshDiagnostics(bridge, doc, diagCollection);
    }
  };

  context.subscriptions.push(
    vscode.workspace.onDidOpenTextDocument((doc) => {
      if (isMojomDocument(doc.languageId)) refreshDiagnostics(bridge, doc, diagCollection);
    }),
    vscode.workspace.onDidChangeTextDocument((e) => {
      if (!isMojomDocument(e.document.languageId)) return;
      const onType = vscode.workspace
        .getConfiguration('mojom')
        .get<boolean>('diagnostics.onType', true);
      if (!onType) return;
      if (debounce) clearTimeout(debounce);
      debounce = setTimeout(() => refreshDiagnostics(bridge, e.document, diagCollection), 150);
    }),
    vscode.workspace.onDidSaveTextDocument((doc) => {
      if (isMojomDocument(doc.languageId)) refreshDiagnostics(bridge, doc, diagCollection);
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
      if (e.affectsConfiguration('mojom.includePaths')) {
        bridge.setIncludePaths(resolveIncludePaths());
        refreshAllOpen();
      }
    }),
    vscode.workspace.onDidChangeWorkspaceFolders(() => {
      bridge.setIncludePaths(resolveIncludePaths());
      void preloadWorkspace(bridge).then(refreshAllOpen);
    }),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand('mojom.restart', async () => {
      diagCollection.clear();
      for (const doc of vscode.workspace.textDocuments) {
        if (isMojomDocument(doc.languageId)) {
          bridge.removeFile(doc.uri.toString());
        }
      }
      bridge.setIncludePaths(resolveIncludePaths());
      await preloadWorkspace(bridge);
      refreshAllOpen();
      vscode.window.showInformationMessage('Mojom analyzer restarted.');
    }),
  );
}

export function deactivate(): void {}
