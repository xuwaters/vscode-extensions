import * as vscode from 'vscode';
import { CONFIG_SECTION, clearApiKey, setApiKey } from './config.js';
import { runCloudflarePreset } from './presets.js';
import { OAICompatChatProvider, VENDOR } from './provider.js';

export function activate(context: vscode.ExtensionContext): void {
  const provider = new OAICompatChatProvider(context.secrets);
  context.subscriptions.push(vscode.lm.registerLanguageModelChatProvider(VENDOR, provider));

  context.subscriptions.push(
    vscode.workspace.onDidChangeConfiguration(e => {
      if (e.affectsConfiguration(CONFIG_SECTION)) {
        provider.fireChange();
      }
    }),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand('wx-openai-compat.addCloudflarePreset', async () => {
      try {
        await runCloudflarePreset(context.secrets);
        provider.fireChange();
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        void vscode.window.showErrorMessage(`OpenAI Compatible: preset failed — ${msg}`);
      }
    }),

    vscode.commands.registerCommand('wx-openai-compat.setApiKey', async () => {
      const value = await vscode.window.showInputBox({
        title: 'OpenAI Compatible — API Key',
        prompt: 'API key sent as Bearer token. Stored in VS Code SecretStorage.',
        password: true,
        ignoreFocusOut: true,
        validateInput: v => (v.trim() ? undefined : 'API key is required'),
      });
      if (!value) return;
      await setApiKey(context.secrets, value.trim());
      provider.fireChange();
      void vscode.window.showInformationMessage('OpenAI Compatible: API key saved.');
    }),

    vscode.commands.registerCommand('wx-openai-compat.clearApiKey', async () => {
      await clearApiKey(context.secrets);
      provider.fireChange();
      void vscode.window.showInformationMessage('OpenAI Compatible: API key cleared.');
    }),

    vscode.commands.registerCommand('wx-openai-compat.refreshModels', () => {
      provider.fireChange();
    }),
  );
}

export function deactivate(): void {}
