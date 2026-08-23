import * as vscode from 'vscode';
import { VimController } from './controller';
import { EngineBridge } from './engine';

export function activate(context: vscode.ExtensionContext): void {
  const bridge = new EngineBridge(context.extensionPath);
  const enabled = vscode.workspace
    .getConfiguration('vimUltra')
    .get<boolean>('enabled', true);
  const controller = new VimController(bridge, enabled);
  context.subscriptions.push(controller);

  // The `type` override receives every printable keystroke while an editor
  // has focus. Another modal extension may already own it; degrade politely.
  try {
    context.subscriptions.push(
      vscode.commands.registerCommand('type', (args: { text: string }) =>
        controller.type(args.text),
      ),
    );
  } catch {
    void vscode.window.showWarningMessage(
      'Vim Ultra: another extension already handles typing (VSCodeVim?). Disable it and reload.',
    );
  }

  context.subscriptions.push(
    vscode.commands.registerCommand('vimUltra.key', (key: string) => controller.key(key)),
    vscode.commands.registerCommand(
      'vimUltra.scroll',
      (args: { dir: 'up' | 'down'; by: 'half' | 'page' }) =>
        controller.scroll(args.dir, args.by),
    ),
    vscode.commands.registerCommand('vimUltra.toggle', () =>
      controller.setEnabled(!controller.isEnabled),
    ),
    vscode.workspace.onDidChangeConfiguration((e) => {
      if (e.affectsConfiguration('vimUltra.enabled')) {
        const on = vscode.workspace
          .getConfiguration('vimUltra')
          .get<boolean>('enabled', true);
        void controller.setEnabled(on);
      }
      // The engine holds its own copy of the EasyMotion settings.
      if (
        e.affectsConfiguration('vimUltra.easyMotion') ||
        e.affectsConfiguration('vimUltra.leader')
      ) {
        controller.reloadSettings();
      }
    }),
  );
}

export function deactivate(): void {
  // Subscriptions dispose the controller, which restores editor state.
}
