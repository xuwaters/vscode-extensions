import * as vscode from 'vscode';
import { AnalyzerBridge } from './analyzer';
import { CONFIG_SECTION } from './config';
import { LANGUAGE_ID, refreshDiagnostics } from './diagnostics';
import { RonDocumentSymbolProvider } from './providers/documentSymbol';
import { RonFoldingRangeProvider } from './providers/foldingRange';
import { RonFormattingProvider } from './providers/formatting';
import { RonHoverProvider } from './providers/hover';
import { RonSelectionRangeProvider } from './providers/selectionRange';

// Untitled buffers too, so a scratch RON document gets the same treatment.
const SELECTOR: vscode.DocumentSelector = [
  { scheme: 'file', language: LANGUAGE_ID },
  { scheme: 'untitled', language: LANGUAGE_ID },
];

const DEBOUNCE_MS = 150;

export function activate(context: vscode.ExtensionContext): void {
  const bridge = AnalyzerBridge.load(context.extensionPath);
  const collection = vscode.languages.createDiagnosticCollection(LANGUAGE_ID);

  context.subscriptions.push(
    collection,
    vscode.languages.registerDocumentSymbolProvider(SELECTOR, new RonDocumentSymbolProvider(bridge)),
    vscode.languages.registerFoldingRangeProvider(SELECTOR, new RonFoldingRangeProvider(bridge)),
    vscode.languages.registerHoverProvider(SELECTOR, new RonHoverProvider(bridge)),
    vscode.languages.registerSelectionRangeProvider(
      SELECTOR,
      new RonSelectionRangeProvider(bridge),
    ),
    vscode.languages.registerDocumentFormattingEditProvider(
      SELECTOR,
      new RonFormattingProvider(bridge),
    ),
  );

  const refresh = (doc: vscode.TextDocument) => refreshDiagnostics(bridge, doc, collection);
  const refreshAll = () => vscode.workspace.textDocuments.forEach(refresh);
  refreshAll();

  const pending = new Map<string, ReturnType<typeof setTimeout>>();
  context.subscriptions.push(
    { dispose: () => pending.forEach(clearTimeout) },
    vscode.workspace.onDidOpenTextDocument(refresh),
    vscode.workspace.onDidChangeTextDocument((e) => {
      if (e.document.languageId !== LANGUAGE_ID) return;
      const key = e.document.uri.toString();
      clearTimeout(pending.get(key));
      pending.set(
        key,
        setTimeout(() => {
          pending.delete(key);
          refresh(e.document);
        }, DEBOUNCE_MS),
      );
    }),
    vscode.workspace.onDidCloseTextDocument((doc) => {
      const key = doc.uri.toString();
      clearTimeout(pending.get(key));
      pending.delete(key);
      collection.delete(doc.uri);
      bridge.removeFile(key);
    }),
    vscode.workspace.onDidChangeConfiguration((e) => {
      if (e.affectsConfiguration(`${CONFIG_SECTION}.diagnostics`)) refreshAll();
    }),
  );
}

export function deactivate(): void {}
