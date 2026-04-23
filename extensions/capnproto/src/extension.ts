import * as vscode from 'vscode';
import { AnalyzerBridge } from './analyzer';
import { isCapnpDocument, refreshDiagnostics } from './diagnostics';
import { CapnpDocumentSymbolProvider } from './providers/documentSymbol';
import { CapnpFoldingRangeProvider } from './providers/foldingRange';

const CAPNP_SELECTOR: vscode.DocumentSelector = { scheme: 'file', language: 'capnp' };

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  const bridge = AnalyzerBridge.load(context.extensionPath);

  const diagCollection = vscode.languages.createDiagnosticCollection('capnp');
  context.subscriptions.push(diagCollection);

  context.subscriptions.push(
    vscode.languages.registerDocumentSymbolProvider(
      CAPNP_SELECTOR,
      new CapnpDocumentSymbolProvider(bridge),
    ),
    vscode.languages.registerFoldingRangeProvider(
      CAPNP_SELECTOR,
      new CapnpFoldingRangeProvider(bridge),
    ),
  );

  for (const doc of vscode.workspace.textDocuments) {
    if (isCapnpDocument(doc.languageId)) {
      refreshDiagnostics(bridge, doc, diagCollection);
    }
  }

  let debounce: ReturnType<typeof setTimeout> | undefined;
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
  );

  context.subscriptions.push(
    vscode.commands.registerCommand('capnp.restart', async () => {
      diagCollection.clear();
      for (const doc of vscode.workspace.textDocuments) {
        if (isCapnpDocument(doc.languageId)) {
          bridge.removeFile(doc.uri.toString());
          refreshDiagnostics(bridge, doc, diagCollection);
        }
      }
      vscode.window.showInformationMessage("Cap'n Proto analyzer restarted.");
    }),
  );
}

export function deactivate(): void {}
