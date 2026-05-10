import * as vscode from 'vscode';
import { LogEditorProvider } from './editorProvider.js';

export function activate(context: vscode.ExtensionContext): void {
  const provider = new LogEditorProvider(context);

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
      provider.toggleActive('renderAnsi');
    }),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand('logViewer.toggleWrap', () => {
      provider.toggleActive('wordWrap');
    }),
  );
}

export function deactivate(): void {}
