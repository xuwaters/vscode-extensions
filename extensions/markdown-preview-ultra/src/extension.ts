import * as vscode from 'vscode';
import { ModeManager } from './modes';
import { PreviewManager, canPreview } from './previewManager';

export function activate(context: vscode.ExtensionContext): void {
  // Suppresses the built-in Markdown preview buttons (editor title bar, explorer
  // and tab context menus) so they don't duplicate ours.
  void vscode.commands.executeCommand(
    'setContext',
    'hasCustomMarkdownPreview',
    true,
  );

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
    vscode.commands.registerCommand('markdownPreviewUltra.navigateBack', () =>
      manager.navigate('back'),
    ),
    vscode.commands.registerCommand('markdownPreviewUltra.navigateForward', () =>
      manager.navigate('forward'),
    ),
    vscode.commands.registerCommand('markdownPreviewUltra.cycleMode', () =>
      modes.cycleMode(),
    ),
    vscode.commands.registerCommand('markdownPreviewUltra.switchMode', () =>
      modes.switchMode(),
    ),
    vscode.commands.registerCommand('markdownPreviewUltra.setModeEdit', () =>
      modes.setMode('edit'),
    ),
    vscode.commands.registerCommand('markdownPreviewUltra.setModeSplit', () =>
      modes.setMode('split'),
    ),
    vscode.commands.registerCommand('markdownPreviewUltra.setModePreview', () =>
      modes.setMode('preview'),
    ),
    vscode.window.registerWebviewPanelSerializer(PreviewManager.viewType, {
      deserializeWebviewPanel: (panel, state) =>
        manager.restorePanel(panel, state as { uri?: string } | undefined),
    }),
  );
}

export function deactivate(): void {}
