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
    vscode.commands.registerCommand('markdownLivePreview.showPreview', () =>
      open(vscode.ViewColumn.Active),
    ),
    vscode.commands.registerCommand('markdownLivePreview.showPreviewToSide', () =>
      open(vscode.ViewColumn.Beside),
    ),
    vscode.commands.registerCommand('markdownLivePreview.toggleFocus', () =>
      manager.toggleFocus(),
    ),
    vscode.commands.registerCommand(
      'markdownLivePreview.togglePreviewLock',
      () => manager.togglePreviewLock(),
    ),
    vscode.commands.registerCommand('markdownLivePreview.cycleMode', () =>
      modes.cycleMode(),
    ),
    vscode.commands.registerCommand('markdownLivePreview.switchMode', () =>
      modes.switchMode(),
    ),
    vscode.window.registerWebviewPanelSerializer(PreviewManager.viewType, {
      deserializeWebviewPanel: (panel, state) =>
        manager.restorePanel(panel, state as { uri?: string } | undefined),
    }),
  );
}

export function deactivate(): void {}
