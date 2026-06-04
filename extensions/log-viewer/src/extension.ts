import * as fs from 'fs';
import * as path from 'path';
import * as vscode from 'vscode';
import { LogEditorProvider } from './editorProvider.js';
import { INDEX_FILE_SUFFIX, globalIndexDir } from './indexer/cache.js';
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
    vscode.commands.registerCommand('logViewer.toggleLineNumbers', () => {
      provider.sendToActive({ type: 'commandToggle', key: 'lineNumbers' });
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

  context.subscriptions.push(
    vscode.commands.registerCommand('logViewer.editFilterSets', () => {
      if (!provider.sendToActive({ type: 'openFilterEditor' })) {
        void vscode.commands.executeCommand(
          'workbench.action.openSettings',
          'logViewer.filterSets',
        );
      }
    }),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand('logViewer.clearIndexCache', async () => {
      // RFC §5.4.4: clear the directories we own, not adjacent files we
      // didn't write this session.
      const cfg = vscode.workspace.getConfiguration('logViewer');
      const dirs: string[] = [globalIndexDir(context.globalStorageUri.fsPath)];
      const customDir = cfg.get<string>('indexDirectory', '');
      if (customDir) dirs.push(customDir);
      let removed = 0;
      for (const dir of dirs) {
        try {
          for (const ent of fs.readdirSync(dir, { withFileTypes: true })) {
            if (ent.isFile() && ent.name.endsWith(INDEX_FILE_SUFFIX)) {
              try {
                fs.unlinkSync(path.join(dir, ent.name));
                removed += 1;
              } catch {
                // ignore
              }
            }
          }
        } catch {
          // directory doesn't exist yet — fine.
        }
      }
      void vscode.window.showInformationMessage(
        `Log Viewer: cleared ${removed} index file${removed === 1 ? '' : 's'}.`,
      );
    }),
  );
}

export function deactivate(): void {}
