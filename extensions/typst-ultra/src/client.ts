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

/** The language server, started lazily and restartable. */
export class Client implements vscode.Disposable {
  private client: LanguageClient | undefined;
  private starting: Promise<void> | undefined;
  private readonly notificationHandlers = new Map<
    string,
    ((params: unknown) => void)[]
  >();

  constructor(
    private readonly context: vscode.ExtensionContext,
    private readonly output: vscode.OutputChannel,
  ) {}

  /** Whether the server is up. */
  get running(): boolean {
    return this.client !== undefined;
  }

  /**
   * Start the server if it is not already running.
   *
   * Lazy on purpose: activation with no `.typ` open should cost nothing, and
   * forking a process plus instantiating 26 MB of WASM is not nothing.
   */
  async start(document?: vscode.Uri): Promise<void> {
    if (this.client) return;
    if (this.starting) return this.starting;

    this.starting = this.doStart(document).finally(() => {
      this.starting = undefined;
    });
    return this.starting;
  }

  /** Stop the server, if it is running. */
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

  /** Send a notification, if the server is running. */
  notify(method: string, params: unknown): void {
    void this.client?.sendNotification(method, params);
  }

  /**
   * Subscribe to a server notification.
   *
   * Registrations survive a restart, which is why they are held here rather
   * than on the client.
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

  dispose(): void {
    void this.stop();
  }

  private async doStart(document?: vscode.Uri): Promise<void> {
    const module = this.context.asAbsolutePath(path.join('dist', 'server.js'));
    if (!fs.existsSync(module)) {
      this.output.appendLine(
        'The server bundle is missing. Run `pnpm run build` in extensions/typst-ultra.',
      );
      return;
    }

    const settings = config.read(document);
    const root = config.resolveRoot(settings, document);

    const serverOptions: ServerOptions = {
      run: { module, transport: TransportKind.ipc },
      debug: {
        module,
        transport: TransportKind.ipc,
        options: { execArgv: ['--nolazy', '--inspect=6019'] },
      },
    };

    const clientOptions: LanguageClientOptions = {
      documentSelector: [{ scheme: 'file', language: 'typst' }],
      outputChannel: this.output,
      // Keep the server alive through a compiler panic: it restarts, and the
      // document version that provoked it is in the log.
      initializationOptions: {
        rootPath: root,
        rootUri: vscode.Uri.file(root).toString(),
        mainPath: this.mainPath(settings, root, document),
        bundledFontsPath: this.context.asAbsolutePath(path.join('assets', 'fonts')),
        fontCachePath: path.join(
          this.context.globalStorageUri.fsPath,
          'font-index.json',
        ),
        extraFontPaths: settings.host.fonts.paths,
        systemFonts: settings.host.fonts.system,
        packages: settings.host.packages,
        settings: settings.server,
      },
      synchronize: {
        fileEvents: vscode.workspace.createFileSystemWatcher('**/*.{typ,typc}'),
      },
    };

    const client = new LanguageClient(
      'typstUltra',
      'Typst Ultra',
      serverOptions,
      clientOptions,
    );

    await client.start();
    this.client = client;

    for (const [method, handlers] of this.notificationHandlers) {
      client.onNotification(method, (params: unknown) => {
        for (const handler of handlers) handler(params);
      });
    }

    // The engine is a build artifact, not a shipped-broken state, so say what
    // to run rather than showing a stack trace.
    client.onNotification('typst/engineMissing', (params: { command: string }) => {
      void vscode.window
        .showErrorMessage(
          `Typst: the engine is not built. Run \`${params.command}\`.`,
          'Show Log',
        )
        .then((choice) => {
          if (choice === 'Show Log') this.output.show(true);
        });
    });
  }

  /** The entry file to compile, root-relative. */
  private mainPath(
    settings: config.Config,
    root: string,
    document?: vscode.Uri,
  ): string {
    const pinned = this.context.workspaceState.get<string>('typstUltra.mainFile');
    const absolute = pinned ?? settings.host.mainFile ?? '';

    if (absolute) {
      return path.isAbsolute(absolute) ? path.relative(root, absolute) : absolute;
    }
    if (document) {
      return path.relative(root, document.fsPath);
    }
    return 'main.typ';
  }
}
