// The language client.
//
// Started lazily and restartable, on the model of typst-ultra's client. The
// server runs in a child process over IPC, so a naga panic on a malformed
// module costs a restart rather than the whole extension host.

import * as fs from 'fs';
import * as path from 'path';
import * as vscode from 'vscode';
import {
  LanguageClient,
  TransportKind,
  type LanguageClientOptions,
  type ServerOptions,
} from 'vscode-languageclient/node';

import * as config from '../config.js';
import { EMBEDDED_SCHEME, hostOf } from './virtualDocuments.js';

export class Client implements vscode.Disposable {
  private client: LanguageClient | undefined;
  private starting: Promise<void> | undefined;
  private readonly notificationHandlers = new Map<string, ((params: unknown) => void)[]>();

  constructor(
    private readonly context: vscode.ExtensionContext,
    private readonly output: vscode.OutputChannel,
  ) {}

  get running(): boolean {
    return this.client !== undefined;
  }

  /**
   * Start the server if it is not already running.
   *
   * Lazy on purpose: activating for a Rust file that turns out to embed no
   * shader should cost nothing, and forking a process plus instantiating the
   * WASM module is not nothing.
   */
  async start(): Promise<void> {
    if (this.client) return;
    if (this.starting) return this.starting;

    this.starting = this.doStart().finally(() => {
      this.starting = undefined;
    });
    return this.starting;
  }

  async stop(): Promise<void> {
    const client = this.client;
    this.client = undefined;
    if (client) await client.stop();
  }

  /** Stop and start again, keeping registered handlers. */
  async restart(): Promise<void> {
    await this.stop();
    await this.start();
  }

  /** Send a request, starting the server first if necessary. */
  async request<T>(method: string, params: unknown): Promise<T | undefined> {
    await this.start();
    if (!this.client) return undefined;
    try {
      return await this.client.sendRequest<T>(method, params);
    } catch (error) {
      this.output.appendLine(`${method} failed: ${String(error)}`);
      return undefined;
    }
  }

  notify(method: string, params: unknown): void {
    void this.client?.sendNotification(method, params);
  }

  /**
   * Subscribe to a server notification.
   *
   * Registrations are held here rather than on the client so they survive a
   * restart.
   */
  onNotification(method: string, handler: (params: unknown) => void): vscode.Disposable {
    const handlers = this.notificationHandlers.get(method) ?? [];
    handlers.push(handler);
    this.notificationHandlers.set(method, handlers);

    return new vscode.Disposable(() => {
      const current = this.notificationHandlers.get(method) ?? [];
      const index = current.indexOf(handler);
      if (index >= 0) current.splice(index, 1);
    });
  }

  /** Push the current settings, without a restart. */
  sendConfiguration(): void {
    this.notify('workspace/didChangeConfiguration', { settings: config.read() });
  }

  dispose(): void {
    void this.stop();
  }

  private async doStart(): Promise<void> {
    const module = this.context.asAbsolutePath(path.join('dist', 'server.js'));
    if (!fs.existsSync(module)) {
      this.output.appendLine(
        'The server bundle is missing. Run `pnpm run build` in extensions/wgsl-shader.',
      );
      return;
    }

    const serverOptions: ServerOptions = {
      run: { module, transport: TransportKind.ipc },
      debug: {
        module,
        transport: TransportKind.ipc,
        options: { execArgv: ['--nolazy', '--inspect=6019'] },
      },
    };

    const clientOptions: LanguageClientOptions = {
      documentSelector: [
        { scheme: 'file', language: 'wgsl' },
        { scheme: 'file', language: 'glsl' },
        { scheme: 'untitled', language: 'wgsl' },
        { scheme: 'untitled', language: 'glsl' },
        // Shaders embedded in Rust and TS/JS, surfaced as virtual documents.
        { scheme: EMBEDDED_SCHEME, language: 'wgsl' },
        { scheme: EMBEDDED_SCHEME, language: 'glsl' },
      ],
      initializationOptions: { settings: config.read() },
      outputChannel: this.output,
      middleware: {
        handleDiagnostics: (uri, diagnostics, next) => {
          next(uri, this.filterDiagnostics(uri, diagnostics));
        },
      },
    };

    const client = new LanguageClient(
      'wgslShader',
      'WGSL / GLSL Shader',
      serverOptions,
      clientOptions,
    );

    for (const [method, handlers] of this.notificationHandlers) {
      client.onNotification(method, (params: unknown) => {
        for (const handler of handlers) handler(params);
      });
    }

    await client.start();
    this.client = client;
  }

  /**
   * Diagnostics for an embedded block, which are off by default.
   *
   * A shader written inside a string is usually a fragment of a program that
   * gets assembled elsewhere — a `#include`-style concatenation, a template
   * that a `${}` fills in — so validating it as a standalone module produces
   * errors about code the author never wrote. The setting is there for people
   * whose embedded shaders *are* complete.
   */
  private filterDiagnostics(
    uri: vscode.Uri,
    diagnostics: vscode.Diagnostic[],
  ): vscode.Diagnostic[] {
    if (uri.scheme !== EMBEDDED_SCHEME) return diagnostics;

    const host = hostOf(uri);
    const language = uri.path.endsWith('.wgsl') ? 'wgsl' : 'glsl';
    const enabled = vscode.workspace
      .getConfiguration(language, host)
      .get<boolean>('embedded.diagnostics', false);
    return enabled ? diagnostics : [];
  }
}
