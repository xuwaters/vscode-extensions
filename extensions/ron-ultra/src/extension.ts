// Activation: start the language server and keep its settings current.
// Every feature — diagnostics, schema validation against the annotated Rust
// type, completion, hover, go-to-definition, formatting, outline, folding and
// smart select — is answered by the server.

import * as vscode from 'vscode';
import type { LanguageClient } from 'vscode-languageclient/node';
import { startClient } from './client.js';
import * as config from './config.js';

let client: LanguageClient | undefined;

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  const output = vscode.window.createOutputChannel('RON Ultra');
  context.subscriptions.push(output);
  client = await startClient(context, output);
  context.subscriptions.push(
    vscode.workspace.onDidChangeConfiguration((e) => {
      if (!e.affectsConfiguration(config.CONFIG_SECTION)) return;
      void client?.sendNotification('workspace/didChangeConfiguration', { settings: config.read() });
    }),
    vscode.commands.registerCommand('ronUltra.restartServer', async () => {
      await client?.stop();
      client = await startClient(context, output);
    }),
  );
}

export async function deactivate(): Promise<void> {
  await client?.stop();
}
