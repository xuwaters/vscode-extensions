import * as vscode from 'vscode';
import { decodeBase64, encodeBase64 } from './base64.js';

function getSelection(): { editor: vscode.TextEditor; selection: vscode.Selection; text: string } | undefined {
  const editor = vscode.window.activeTextEditor;
  if (!editor) {
    return undefined;
  }
  const selection = editor.selection;
  if (selection.isEmpty) {
    vscode.window.showWarningMessage('Base64 Tools: No text selected.');
    return undefined;
  }
  return { editor, selection, text: editor.document.getText(selection) };
}

export function activate(context: vscode.ExtensionContext): void {
  context.subscriptions.push(
    vscode.commands.registerCommand('base64-tools.encode', async () => {
      const result = getSelection();
      if (!result) return;
      const encoded = encodeBase64(result.text);
      await result.editor.edit(b => b.replace(result.selection, encoded));
    }),

    vscode.commands.registerCommand('base64-tools.decode', async () => {
      const result = getSelection();
      if (!result) return;
      const decoded = decodeBase64(result.text);
      if (!decoded.ok) {
        vscode.window.showErrorMessage(`Base64 Tools: ${decoded.error}`);
        return;
      }
      await result.editor.edit(b => b.replace(result.selection, decoded.value));
    }),
  );
}

export function deactivate(): void {}
