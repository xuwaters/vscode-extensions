import * as vscode from 'vscode';
import { AnalyzerBridge } from './analyzer';
import { refreshDiagnostics } from './diagnostics';
import { MakefileDocumentSymbolProvider } from './providers/documentSymbol';
import { MakefileFoldingRangeProvider } from './providers/foldingRange';

const MAKEFILE_SELECTOR: vscode.DocumentSelector = { scheme: 'file', language: 'makefile' };

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  const bridge = AnalyzerBridge.load(context.extensionPath);

  const diagCollection = vscode.languages.createDiagnosticCollection('makefile');
  context.subscriptions.push(diagCollection);

  context.subscriptions.push(
    vscode.languages.registerDocumentSymbolProvider(
      MAKEFILE_SELECTOR,
      new MakefileDocumentSymbolProvider(bridge),
    ),
    vscode.languages.registerFoldingRangeProvider(
      MAKEFILE_SELECTOR,
      new MakefileFoldingRangeProvider(bridge),
    ),
  );

  for (const doc of vscode.workspace.textDocuments) {
    if (doc.languageId === 'makefile') {
      refreshDiagnostics(bridge, doc, diagCollection);
    }
  }

  let debounce: ReturnType<typeof setTimeout> | undefined;
  context.subscriptions.push(
    vscode.workspace.onDidOpenTextDocument((doc) => {
      if (doc.languageId === 'makefile') refreshDiagnostics(bridge, doc, diagCollection);
    }),
    vscode.workspace.onDidChangeTextDocument((e) => {
      if (e.document.languageId !== 'makefile') return;
      const onType = vscode.workspace
        .getConfiguration('makefile')
        .get<boolean>('diagnostics.onType', true);
      if (!onType) return;
      if (debounce) clearTimeout(debounce);
      debounce = setTimeout(() => refreshDiagnostics(bridge, e.document, diagCollection), 150);
    }),
    vscode.workspace.onDidSaveTextDocument((doc) => {
      if (doc.languageId === 'makefile') refreshDiagnostics(bridge, doc, diagCollection);
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
      if (e.affectsConfiguration('makefile.diagnostics.enabled')) {
        for (const doc of vscode.workspace.textDocuments) {
          if (doc.languageId === 'makefile') refreshDiagnostics(bridge, doc, diagCollection);
        }
      }
    }),
  );
}

export function deactivate(): void {}
