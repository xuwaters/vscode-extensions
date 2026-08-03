import * as vscode from 'vscode';
import { AnalyzerBridge } from './analyzer';
import { refreshDiagnostics } from './diagnostics';
import { DotenvCompletionProvider } from './providers/completion';
import { DotenvDocumentSymbolProvider } from './providers/documentSymbol';
import { DotenvFoldingRangeProvider } from './providers/foldingRange';
import { DotenvFormattingProvider } from './providers/formatting';
import { DotenvHoverProvider } from './providers/hover';

const DOTENV_SELECTOR: vscode.DocumentSelector = { scheme: 'file', language: 'dotenv' };

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  const bridge = AnalyzerBridge.load(context.extensionPath);

  const diagCollection = vscode.languages.createDiagnosticCollection('dotenv');
  context.subscriptions.push(diagCollection);

  const formattingProvider = new DotenvFormattingProvider(bridge);
  context.subscriptions.push(
    vscode.languages.registerDocumentSymbolProvider(
      DOTENV_SELECTOR,
      new DotenvDocumentSymbolProvider(bridge),
    ),
    vscode.languages.registerFoldingRangeProvider(
      DOTENV_SELECTOR,
      new DotenvFoldingRangeProvider(bridge),
    ),
    vscode.languages.registerCompletionItemProvider(
      DOTENV_SELECTOR,
      new DotenvCompletionProvider(bridge),
      '$',
      '{',
    ),
    vscode.languages.registerHoverProvider(DOTENV_SELECTOR, new DotenvHoverProvider(bridge)),
    vscode.languages.registerDocumentFormattingEditProvider(DOTENV_SELECTOR, formattingProvider),
    vscode.languages.registerDocumentRangeFormattingEditProvider(
      DOTENV_SELECTOR,
      formattingProvider,
    ),
  );

  for (const doc of vscode.workspace.textDocuments) {
    if (doc.languageId === 'dotenv') {
      refreshDiagnostics(bridge, doc, diagCollection);
    }
  }

  let debounce: ReturnType<typeof setTimeout> | undefined;
  context.subscriptions.push(
    vscode.workspace.onDidOpenTextDocument((doc) => {
      if (doc.languageId === 'dotenv') refreshDiagnostics(bridge, doc, diagCollection);
    }),
    vscode.workspace.onDidChangeTextDocument((e) => {
      if (e.document.languageId !== 'dotenv') return;
      const onType = vscode.workspace
        .getConfiguration('dotenv')
        .get<boolean>('diagnostics.onType', true);
      if (!onType) return;
      if (debounce) clearTimeout(debounce);
      debounce = setTimeout(() => refreshDiagnostics(bridge, e.document, diagCollection), 150);
    }),
    vscode.workspace.onDidSaveTextDocument((doc) => {
      if (doc.languageId === 'dotenv') refreshDiagnostics(bridge, doc, diagCollection);
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
      if (e.affectsConfiguration('dotenv.diagnostics.enabled')) {
        for (const doc of vscode.workspace.textDocuments) {
          if (doc.languageId === 'dotenv') refreshDiagnostics(bridge, doc, diagCollection);
        }
      }
    }),
  );
}

export function deactivate(): void {}
