import * as vscode from 'vscode';
import { AnalyzerBridge } from './analyzer';
import { refreshDiagnostics } from './diagnostics';
import { DieselCompletionProvider } from './providers/completion';
import { DieselDocumentSymbolProvider } from './providers/documentSymbol';
import { DieselFoldingRangeProvider } from './providers/foldingRange';
import { DieselHoverProvider } from './providers/hover';
import { isDieselSchema } from './util';

const RUST_SELECTOR: vscode.DocumentSelector = { scheme: 'file', language: 'rust' };

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  if (!vscode.workspace.getConfiguration('dieselSchema').get<boolean>('enabled', true)) {
    return;
  }

  const bridge = AnalyzerBridge.load(context.extensionPath);

  const diagCollection = vscode.languages.createDiagnosticCollection('dieselSchema');
  context.subscriptions.push(diagCollection);

  context.subscriptions.push(
    vscode.languages.registerDocumentSymbolProvider(
      RUST_SELECTOR,
      new DieselDocumentSymbolProvider(bridge),
    ),
    vscode.languages.registerFoldingRangeProvider(
      RUST_SELECTOR,
      new DieselFoldingRangeProvider(bridge),
    ),
    vscode.languages.registerCompletionItemProvider(
      RUST_SELECTOR,
      new DieselCompletionProvider(bridge),
    ),
    vscode.languages.registerHoverProvider(RUST_SELECTOR, new DieselHoverProvider(bridge)),
  );

  for (const doc of vscode.workspace.textDocuments) {
    if (isDieselSchema(doc)) {
      refreshDiagnostics(bridge, doc, diagCollection);
    }
  }

  const debounceByUri = new Map<string, ReturnType<typeof setTimeout>>();
  context.subscriptions.push(
    vscode.workspace.onDidOpenTextDocument((doc) => {
      if (isDieselSchema(doc)) refreshDiagnostics(bridge, doc, diagCollection);
    }),
    vscode.workspace.onDidChangeTextDocument((e) => {
      if (!isDieselSchema(e.document)) return;
      const onType = vscode.workspace
        .getConfiguration('dieselSchema')
        .get<boolean>('diagnostics.onType', true);
      if (!onType) return;
      const key = e.document.uri.toString();
      const prev = debounceByUri.get(key);
      if (prev) clearTimeout(prev);
      debounceByUri.set(
        key,
        setTimeout(() => refreshDiagnostics(bridge, e.document, diagCollection), 200),
      );
    }),
    vscode.workspace.onDidSaveTextDocument((doc) => {
      if (isDieselSchema(doc)) refreshDiagnostics(bridge, doc, diagCollection);
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
      if (
        e.affectsConfiguration('dieselSchema.diagnostics.enabled') ||
        e.affectsConfiguration('dieselSchema.enabled')
      ) {
        for (const doc of vscode.workspace.textDocuments) {
          if (isDieselSchema(doc)) refreshDiagnostics(bridge, doc, diagCollection);
        }
      }
    }),
  );
}

export function deactivate(): void {}
