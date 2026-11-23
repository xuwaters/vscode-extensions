// The language client. The server runs in a child process over IPC, so the
// WASM heap and any panic stay out of the extension host.

import * as fs from 'fs';
import * as path from 'path';
import * as vscode from 'vscode';
import {
  LanguageClient,
  TransportKind,
  type LanguageClientOptions,
  type ServerOptions,
} from 'vscode-languageclient/node';
import * as config from './config.js';

export const LANGUAGE_ID = 'ron';

export async function startClient(
  context: vscode.ExtensionContext,
  output: vscode.OutputChannel,
): Promise<LanguageClient | undefined> {
  const module = context.asAbsolutePath(path.join('dist', 'server.js'));
  if (!fs.existsSync(module)) {
    output.appendLine('The server bundle is missing. Run `pnpm run build` in extensions/ron-ultra.');
    return undefined;
  }
  const serverOptions: ServerOptions = {
    run: { module, transport: TransportKind.ipc },
    debug: {
      module,
      transport: TransportKind.ipc,
      options: { execArgv: ['--nolazy', '--inspect=6021'] },
    },
  };
  const clientOptions: LanguageClientOptions = {
    // Untitled buffers too, so a scratch RON document gets the same treatment.
    documentSelector: [
      { scheme: 'file', language: LANGUAGE_ID },
      { scheme: 'untitled', language: LANGUAGE_ID },
    ],
    initializationOptions: { settings: config.read() },
    outputChannel: output,
    synchronize: {
      // A `@[crate::...]` annotation is checked against Rust sources the
      // server reads from disk; it drops what it cached when they change.
      fileEvents: vscode.workspace.createFileSystemWatcher('**/{*.rs,Cargo.toml}'),
    },
  };
  const client = new LanguageClient('ronUltra', 'RON Ultra', serverOptions, clientOptions);
  await client.start();
  return client;
}
