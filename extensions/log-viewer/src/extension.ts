import * as vscode from 'vscode';
import { LogEditorProvider } from './editorProvider.js';
import { loadWasm } from './wasm.js';

export function activate(context: vscode.ExtensionContext): void {
  const wasm = loadWasm(context.extensionPath);
  const provider = new LogEditorProvider(context, wasm);

  context.subscriptions.push(
    vscode.window.registerCustomEditorProvider(
      LogEditorProvider.viewType,
      provider,
      {
        webviewOptions: { retainContextWhenHidden: true },
        supportsMultipleEditorsPerDocument: false,
      },
    ),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand('logViewer.openInLogViewer', async () => {
      const editor = vscode.window.activeTextEditor;
      if (!editor) return;
      await vscode.commands.executeCommand(
        'vscode.openWith',
        editor.document.uri,
        LogEditorProvider.viewType,
      );
    }),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand('logViewer.openInTextEditor', async () => {
      const editor = vscode.window.activeTextEditor;
      const uri = editor?.document.uri;
      if (!uri) return;
      await vscode.commands.executeCommand('vscode.openWith', uri, 'default');
    }),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand('logViewer.toggleAnsi', () => {
      provider.sendToActive({ type: 'commandToggle', key: 'renderAnsi' });
    }),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand('logViewer.toggleWrap', () => {
      provider.sendToActive({ type: 'commandToggle', key: 'wordWrap' });
    }),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand('logViewer.toggleFilterMode', () => {
      provider.sendToActive({ type: 'commandToggle', key: 'filterMode' });
    }),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand('logViewer.focusSearch', () => {
      provider.sendToActive({ type: 'focusSearch' });
    }),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand('logViewer.fontSizeIncrease', () => {
      provider.sendToActive({ type: 'fontSizeCommand', delta: 1 });
    }),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand('logViewer.fontSizeDecrease', () => {
      provider.sendToActive({ type: 'fontSizeCommand', delta: -1 });
    }),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand('logViewer.fontSizeReset', () => {
      provider.sendToActive({ type: 'fontSizeCommand', delta: 'reset' });
    }),
  );
}

export function deactivate(): void {}
