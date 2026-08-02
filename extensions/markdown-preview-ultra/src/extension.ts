import * as vscode from 'vscode';
import { MarkdownEditorProvider, isPreviewable } from './customEditor';
import { ModeManager } from './modes';
import { PreviewManager, canPreview } from './previewManager';
import { PreviewRenderer } from './renderer';

export function activate(context: vscode.ExtensionContext): void {
  // Suppresses the built-in Markdown preview buttons (editor title bar, explorer
  // and tab context menus) so they don't duplicate ours.
  void vscode.commands.executeCommand(
    'setContext',
    'hasCustomMarkdownPreview',
    true,
  );

  // Shared by both surfaces, so the WASM engine is loaded at most once.
  const renderer = new PreviewRenderer(context.extensionUri);
  const manager = new PreviewManager(renderer);
  const editors = new MarkdownEditorProvider(renderer);
  const modes = new ModeManager(manager, editors);
  context.subscriptions.push(manager, modes);

  /**
   * Resolve what to preview. The explorer and tab context menus pass the
   * resource that was clicked; the palette and keybindings pass nothing and
   * mean the active editor.
   */
  const open = async (
    column: vscode.ViewColumn,
    uri?: vscode.Uri,
  ): Promise<void> => {
    if (uri && isPreviewable(uri)) {
      manager.showPreview(await vscode.workspace.openTextDocument(uri), column);
      return;
    }
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
    vscode.commands.registerCommand(
      'markdownPreviewUltra.showPreview',
      (uri?: vscode.Uri) => open(vscode.ViewColumn.Active, uri),
    ),
    vscode.commands.registerCommand(
      'markdownPreviewUltra.showPreviewToSide',
      (uri?: vscode.Uri) => open(vscode.ViewColumn.Beside, uri),
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
    // The full-tab preview: what Preview mode swaps the source editor for, and
    // — with `workbench.editorAssociations` pointing `*.md` here — what a
    // markdown file opens as, with no flash of source first.
    vscode.window.registerCustomEditorProvider(
      MarkdownEditorProvider.viewType,
      editors,
      {
        webviewOptions: {
          retainContextWhenHidden: true,
          enableFindWidget: true,
        },
        supportsMultipleEditorsPerDocument: false,
      },
    ),
  );
}

export function deactivate(): void {}
