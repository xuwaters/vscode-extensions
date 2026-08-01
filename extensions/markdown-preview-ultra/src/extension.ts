import * as vscode from 'vscode';
import { ModeManager } from './modes';
import { PreviewManager, canPreview } from './previewManager';

export function activate(context: vscode.ExtensionContext): void {
  const manager = new PreviewManager(context);
  const modes = new ModeManager(manager);
  context.subscriptions.push(manager, modes);

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
    vscode.commands.registerCommand('markdownPreviewUltra.showPreview', () =>
      open(vscode.ViewColumn.Active),
    ),
    vscode.commands.registerCommand('markdownPreviewUltra.showPreviewToSide', () =>
      open(vscode.ViewColumn.Beside),
    ),
    vscode.commands.registerCommand('markdownPreviewUltra.toggleFocus', () =>
      manager.toggleFocus(),
    ),
    vscode.commands.registerCommand(
      'markdownPreviewUltra.togglePreviewLock',
      () => manager.togglePreviewLock(),
    ),
    vscode.commands.registerCommand('markdownPreviewUltra.cycleMode', () =>
      modes.cycleMode(),
    ),
    vscode.commands.registerCommand('markdownPreviewUltra.switchMode', () =>
      modes.switchMode(),
    ),
    vscode.window.registerWebviewPanelSerializer(PreviewManager.viewType, {
      deserializeWebviewPanel: (panel, state) =>
        manager.restorePanel(panel, state as { uri?: string } | undefined),
    }),
  );
}

export function deactivate(): void {}
