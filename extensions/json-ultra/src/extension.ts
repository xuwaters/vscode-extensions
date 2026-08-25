import * as vscode from 'vscode';
import { AnalyzerBridge } from './analyzer.js';
import { formatOptionsFor } from './config.js';
import { DIAGNOSTIC_LANGUAGES, refreshDiagnostics } from './diagnostics.js';
import { JsonlPreviewProvider } from './preview/provider.js';
import { JsonDocumentSymbolProvider } from './providers/documentSymbol.js';
import { JsonFoldingRangeProvider } from './providers/foldingRange.js';
import { JsonFormattingProvider, withDocumentEol, fullRange } from './providers/formatting.js';
import { JsonHoverProvider } from './providers/hover.js';

/** Formatting (and the sort command) covers the whole family. */
const ALL_LANGUAGES = ['json', 'jsonc', 'json5', 'jsonl'] as const;

/** Structure providers only where VSCode's built-in JSON service is absent. */
const OWN_LANGUAGES = ['json5', 'jsonl'] as const;

const DIAGNOSTIC_DEBOUNCE_MS = 150;

function selectors(languages: readonly string[]): vscode.DocumentSelector {
  return languages.map((language) => ({ language }));
}

export function activate(context: vscode.ExtensionContext): void {
  const output = vscode.window.createOutputChannel('JSON Ultra');
  context.subscriptions.push(output);
  const log = (message: string) => output.appendLine(message);

  const bridge = AnalyzerBridge.load(context.extensionPath);
  if (!bridge.available) {
    log('WASM analyzer bundle missing — language features are disabled.');
  }

  const formattingProvider = new JsonFormattingProvider(bridge, log);
  context.subscriptions.push(
    vscode.languages.registerDocumentFormattingEditProvider(
      selectors(ALL_LANGUAGES),
      formattingProvider,
    ),
    vscode.languages.registerDocumentSymbolProvider(
      selectors(OWN_LANGUAGES),
      new JsonDocumentSymbolProvider(bridge),
    ),
    vscode.languages.registerFoldingRangeProvider(
      selectors(OWN_LANGUAGES),
      new JsonFoldingRangeProvider(bridge),
    ),
    vscode.languages.registerHoverProvider(
      selectors(OWN_LANGUAGES),
      new JsonHoverProvider(bridge),
    ),
    JsonlPreviewProvider.register(context, bridge, log),
  );

  wireDiagnostics(context, bridge);
  registerCommands(context, bridge, output);
}

function wireDiagnostics(context: vscode.ExtensionContext, bridge: AnalyzerBridge): void {
  const collection = vscode.languages.createDiagnosticCollection('json-ultra');
  context.subscriptions.push(collection);

  const timers = new Map<string, ReturnType<typeof setTimeout>>();
  const schedule = (document: vscode.TextDocument) => {
    const key = document.uri.toString();
    const existing = timers.get(key);
    if (existing !== undefined) clearTimeout(existing);
    timers.set(
      key,
      setTimeout(() => {
        timers.delete(key);
        refreshDiagnostics(collection, bridge, document);
      }, DIAGNOSTIC_DEBOUNCE_MS),
    );
  };

  for (const document of vscode.workspace.textDocuments) {
    refreshDiagnostics(collection, bridge, document);
  }
  context.subscriptions.push(
    vscode.workspace.onDidOpenTextDocument((document) => {
      refreshDiagnostics(collection, bridge, document);
    }),
    vscode.workspace.onDidChangeTextDocument((event) => {
      if (!DIAGNOSTIC_LANGUAGES.has(event.document.languageId)) return;
      schedule(event.document);
    }),
    vscode.workspace.onDidSaveTextDocument((document) => {
      refreshDiagnostics(collection, bridge, document);
    }),
    vscode.workspace.onDidCloseTextDocument((document) => {
      collection.delete(document.uri);
      bridge.removeFile(document.uri.toString());
    }),
    vscode.workspace.onDidChangeConfiguration((event) => {
      if (!event.affectsConfiguration('jsonUltra.diagnostics')) return;
      for (const document of vscode.workspace.textDocuments) {
        refreshDiagnostics(collection, bridge, document);
      }
    }),
  );
}

function registerCommands(
  context: vscode.ExtensionContext,
  bridge: AnalyzerBridge,
  output: vscode.OutputChannel,
): void {
  context.subscriptions.push(
    vscode.commands.registerCommand('jsonUltra.sortKeys', async () => {
      const editor = vscode.window.activeTextEditor;
      if (!editor) return;
      const document = editor.document;
      if (!(ALL_LANGUAGES as readonly string[]).includes(document.languageId)) {
        void vscode.window.showInformationMessage(
          'JSON Ultra: the active editor is not a JSON document.',
        );
        return;
      }
      const uri = document.uri.toString();
      bridge.updateFile(uri, document.getText(), document.languageId);
      const options = formatOptionsFor(
        document,
        {
          tabSize: typeof editor.options.tabSize === 'number' ? editor.options.tabSize : 2,
          insertSpaces: editor.options.insertSpaces !== false,
        },
        true,
      );
      const edit = bridge.sortKeys(uri, options);
      if (!edit) {
        void vscode.window.showInformationMessage(
          'JSON Ultra: nothing to sort — the document is either already sorted and formatted, or has syntax errors.',
        );
        return;
      }
      await editor.edit((builder) => {
        builder.replace(fullRange(document), withDocumentEol(document, edit.new_text));
      });
    }),

    vscode.commands.registerCommand('jsonUltra.openPreview', async (uri?: vscode.Uri) => {
      const target = uri ?? vscode.window.activeTextEditor?.document.uri;
      if (!target) return;
      await vscode.commands.executeCommand(
        'vscode.openWith',
        target,
        JsonlPreviewProvider.viewType,
      );
    }),

    vscode.commands.registerCommand('jsonUltra.openPreviewToSide', async (uri?: vscode.Uri) => {
      const target = uri ?? vscode.window.activeTextEditor?.document.uri;
      if (!target) return;
      await vscode.commands.executeCommand(
        'vscode.openWith',
        target,
        JsonlPreviewProvider.viewType,
        { viewColumn: vscode.ViewColumn.Beside },
      );
    }),

    vscode.commands.registerCommand('jsonUltra.openInTextEditor', async (uri?: vscode.Uri) => {
      const target = uri ?? vscode.window.activeTextEditor?.document.uri;
      // Inside the custom editor there is no active text editor; VSCode
      // passes the resource to title-bar commands, so `uri` is set there.
      if (!target) return;
      await vscode.commands.executeCommand('vscode.openWith', target, 'default');
    }),

    vscode.commands.registerCommand('jsonUltra.showLog', () => {
      output.show(true);
    }),
  );
}

export function deactivate(): void {
  // Everything lives in context.subscriptions.
}
