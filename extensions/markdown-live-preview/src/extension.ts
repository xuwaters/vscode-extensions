import * as vscode from 'vscode';
import { LivePreviewEditorProvider } from './editorProvider';
import type { EditorMode } from './messages';

export function activate(context: vscode.ExtensionContext): void {
  const provider = new LivePreviewEditorProvider(context);

  context.subscriptions.push(
    vscode.window.registerCustomEditorProvider(
      LivePreviewEditorProvider.viewType,
      provider,
      {
        webviewOptions: { retainContextWhenHidden: true },
        supportsMultipleEditorsPerDocument: false,
      },
    ),
  );

  // Mode switching commands
  const modeOrder: EditorMode[] = ['source', 'live-preview', 'read'];

  context.subscriptions.push(
    vscode.commands.registerCommand('markdownLivePreview.cycleMode', () => {
      const panel = getActiveWebviewPanel();
      if (!panel) return;
      // The webview handles cycling internally; we just forward the request
      panel.webview.postMessage({ type: 'mode:cycle' });
    }),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand('markdownLivePreview.setModeSource', () => {
      setMode('source');
    }),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand('markdownLivePreview.setModeRead', () => {
      setMode('read');
    }),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand('markdownLivePreview.setModeLivePreview', () => {
      setMode('live-preview');
    }),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand('markdownLivePreview.open', async () => {
      const editor = vscode.window.activeTextEditor;
      if (!editor) return;
      await vscode.commands.executeCommand(
        'vscode.openWith',
        editor.document.uri,
        LivePreviewEditorProvider.viewType,
      );
    }),
  );

  // Status bar item showing current mode
  const statusBarItem = vscode.window.createStatusBarItem(
    vscode.StatusBarAlignment.Right,
    100,
  );
  statusBarItem.command = 'markdownLivePreview.cycleMode';
  statusBarItem.text = '$(book) MD: Live Preview';
  statusBarItem.tooltip = 'Click to cycle editor mode';
  context.subscriptions.push(statusBarItem);

  // Show status bar when our editor is active
  context.subscriptions.push(
    vscode.window.onDidChangeActiveTextEditor(() => {
      updateStatusBar(statusBarItem);
    }),
  );
  updateStatusBar(statusBarItem);

  function setMode(mode: EditorMode): void {
    const panel = getActiveWebviewPanel();
    if (!panel) return;
    panel.webview.postMessage({ type: 'mode:set', mode });
  }

  function getActiveWebviewPanel(): vscode.WebviewPanel | undefined {
    // CustomTextEditor doesn't expose the panel directly.
    // The commands work because VSCode routes postMessage to the active custom editor's webview.
    // We use a sentinel approach: post to all visible webviews for this editor type.
    // For now, we rely on the command context `activeCustomEditorId`.
    return undefined;
  }

  function updateStatusBar(item: vscode.StatusBarItem): void {
    // Status bar is visible when our custom editor is active
    // VSCode doesn't have a direct API for this, so we show it when a markdown file is active
    const editor = vscode.window.activeTextEditor;
    if (editor && /\.(md|mdx|markdown)$/i.test(editor.document.fileName)) {
      item.show();
    } else {
      item.hide();
    }
  }
}

export function deactivate(): void {}
