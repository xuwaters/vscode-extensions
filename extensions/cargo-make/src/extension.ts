import * as vscode from 'vscode';
import { AnalyzerBridge } from './analyzer';
import { isCargoMakeDocument, refreshDiagnostics } from './diagnostics';
import { CargoMakeCompletionProvider } from './providers/completion';
import { CargoMakeDefinitionProvider } from './providers/definition';
import { CargoMakeDocumentSymbolProvider } from './providers/documentSymbol';
import { CargoMakeFoldingRangeProvider } from './providers/foldingRange';
import { CargoMakeHoverProvider } from './providers/hover';

const SELECTOR: vscode.DocumentSelector = { scheme: 'file', language: 'cargo-make' };

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  const bridge = AnalyzerBridge.load(context.extensionPath);

  const diagCollection = vscode.languages.createDiagnosticCollection('cargo-make');
  context.subscriptions.push(diagCollection);

  context.subscriptions.push(
    vscode.languages.registerDocumentSymbolProvider(
      SELECTOR,
      new CargoMakeDocumentSymbolProvider(bridge),
    ),
    vscode.languages.registerFoldingRangeProvider(
      SELECTOR,
      new CargoMakeFoldingRangeProvider(bridge),
    ),
    vscode.languages.registerHoverProvider(SELECTOR, new CargoMakeHoverProvider(bridge)),
    vscode.languages.registerDefinitionProvider(SELECTOR, new CargoMakeDefinitionProvider(bridge)),
    vscode.languages.registerCompletionItemProvider(
      SELECTOR,
      new CargoMakeCompletionProvider(bridge),
      '"',
      '@',
      ' ',
    ),
  );

  for (const doc of vscode.workspace.textDocuments) {
    if (isCargoMakeDocument(doc.languageId)) {
      refreshDiagnostics(bridge, doc, diagCollection);
    }
  }

  let debounce: ReturnType<typeof setTimeout> | undefined;
  context.subscriptions.push(
    vscode.workspace.onDidOpenTextDocument((doc) => {
      if (isCargoMakeDocument(doc.languageId)) refreshDiagnostics(bridge, doc, diagCollection);
    }),
    vscode.workspace.onDidChangeTextDocument((e) => {
      if (!isCargoMakeDocument(e.document.languageId)) return;
      const onType = vscode.workspace
        .getConfiguration('cargoMake')
        .get<boolean>('diagnostics.onType', true);
      if (!onType) return;
      if (debounce) clearTimeout(debounce);
      debounce = setTimeout(() => refreshDiagnostics(bridge, e.document, diagCollection), 150);
    }),
    vscode.workspace.onDidSaveTextDocument((doc) => {
      if (isCargoMakeDocument(doc.languageId)) refreshDiagnostics(bridge, doc, diagCollection);
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
      if (e.affectsConfiguration('cargoMake.diagnostics.enabled')) {
        for (const doc of vscode.workspace.textDocuments) {
          if (isCargoMakeDocument(doc.languageId)) {
            refreshDiagnostics(bridge, doc, diagCollection);
          }
        }
      }
    }),
  );
}

export function deactivate(): void {}
