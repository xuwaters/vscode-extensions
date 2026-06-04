import * as vscode from 'vscode';
import { PreviewManager, canPreview } from './previewManager';

export function activate(context: vscode.ExtensionContext): void {
  const manager = new PreviewManager(context);
  context.subscriptions.push(manager);

  const open = (column: vscode.ViewColumn) => {
    const editor = vscode.window.activeTextEditor;
    if (!canPreview(editor?.document)) {
      vscode.window.showInformationMessage(
        'Open a Markdown file to show its preview.',
      );
      return;
    }
    manager.showPreview(editor.document, column);
  };

  context.subscriptions.push(
    vscode.commands.registerCommand('markdownLivePreview.showPreview', () =>
      open(vscode.ViewColumn.Active),
    ),
    vscode.commands.registerCommand('markdownLivePreview.showPreviewToSide', () =>
      open(vscode.ViewColumn.Beside),
    ),
  );
}

export function deactivate(): void {}
