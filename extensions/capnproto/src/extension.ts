import * as vscode from 'vscode';
import { AnalyzerBridge } from './analyzer';
import { isCapnpDocument, refreshDiagnostics } from './diagnostics';
import { resolveIncludePaths } from './includePaths';
import { preloadWorkspace } from './workspaceBootstrap';
import { CapnpDocumentSymbolProvider } from './providers/documentSymbol';
import { CapnpFoldingRangeProvider } from './providers/foldingRange';
import { CapnpHoverProvider } from './providers/hover';
import { CapnpDefinitionProvider } from './providers/definition';
import { CapnpCompletionProvider } from './providers/completion';
import { CapnpWorkspaceSymbolProvider } from './providers/workspaceSymbol';

const CAPNP_SELECTOR: vscode.DocumentSelector = { scheme: 'file', language: 'capnp' };

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  const bridge = AnalyzerBridge.load(context.extensionPath);

  const diagCollection = vscode.languages.createDiagnosticCollection('capnp');
  context.subscriptions.push(diagCollection);

  bridge.setIncludePaths(resolveIncludePaths());
  await preloadWorkspace(bridge);

  context.subscriptions.push(
    vscode.languages.registerDocumentSymbolProvider(
      CAPNP_SELECTOR,
      new CapnpDocumentSymbolProvider(bridge),
    ),
    vscode.languages.registerFoldingRangeProvider(
      CAPNP_SELECTOR,
      new CapnpFoldingRangeProvider(bridge),
    ),
    vscode.languages.registerHoverProvider(CAPNP_SELECTOR, new CapnpHoverProvider(bridge)),
    vscode.languages.registerDefinitionProvider(
      CAPNP_SELECTOR,
      new CapnpDefinitionProvider(bridge),
    ),
    vscode.languages.registerCompletionItemProvider(
      CAPNP_SELECTOR,
      new CapnpCompletionProvider(bridge),
      '.',
      ':',
    ),
    vscode.languages.registerWorkspaceSymbolProvider(new CapnpWorkspaceSymbolProvider(bridge)),
  );

  for (const doc of vscode.workspace.textDocuments) {
    if (isCapnpDocument(doc.languageId)) {
      refreshDiagnostics(bridge, doc, diagCollection);
    }
  }

  let debounce: ReturnType<typeof setTimeout> | undefined;
  const refreshAllOpen = () => {
    for (const doc of vscode.workspace.textDocuments) {
      if (isCapnpDocument(doc.languageId)) refreshDiagnostics(bridge, doc, diagCollection);
    }
  };

  context.subscriptions.push(
    vscode.workspace.onDidOpenTextDocument((doc) => {
      if (isCapnpDocument(doc.languageId)) refreshDiagnostics(bridge, doc, diagCollection);
    }),
    vscode.workspace.onDidChangeTextDocument((e) => {
      if (!isCapnpDocument(e.document.languageId)) return;
      const onType = vscode.workspace
        .getConfiguration('capnp')
        .get<boolean>('diagnostics.onType', true);
      if (!onType) return;
      if (debounce) clearTimeout(debounce);
      debounce = setTimeout(() => refreshDiagnostics(bridge, e.document, diagCollection), 150);
    }),
    vscode.workspace.onDidSaveTextDocument((doc) => {
      if (isCapnpDocument(doc.languageId)) refreshDiagnostics(bridge, doc, diagCollection);
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
      if (e.affectsConfiguration('capnp.includePaths')) {
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
    vscode.commands.registerCommand('capnp.restart', async () => {
      diagCollection.clear();
      for (const doc of vscode.workspace.textDocuments) {
        if (isCapnpDocument(doc.languageId)) {
          bridge.removeFile(doc.uri.toString());
        }
      }
      bridge.setIncludePaths(resolveIncludePaths());
      await preloadWorkspace(bridge);
      refreshAllOpen();
      vscode.window.showInformationMessage("Cap'n Proto analyzer restarted.");
    }),
  );
}

export function deactivate(): void {}
