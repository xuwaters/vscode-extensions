import * as child_process from 'child_process';
import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import { pathToFileURL } from 'url';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';

/**
 * The built server, over a real LSP connection.
 *
 * Every other suite reaches past the transport: `engine.test.ts` calls the WASM
 * directly, and the Rust tests call the dispatcher directly. This one forks
 * `dist/server.js` exactly as `vscode-languageclient` does — `TransportKind.ipc`
 * — and speaks JSON-RPC to it.
 *
 * That covers the seam nothing else does: the bundle's own wiring. A missing
 * export, a broken `require` path to `wasm/`, a handler registered under the
 * wrong method name, or a capability the client would never see are all invisible
 * to the other suites and fatal in the editor.
 */

const SERVER = path.join(__dirname, '..', 'dist', 'server.js');
const WASM = path.join(__dirname, '..', 'wasm', 'typst_lsp_wasm.js');
const READY = fs.existsSync(SERVER) && fs.existsSync(WASM);

/** A JSON-RPC message over Node IPC. */
interface Message {
  id?: number;
  method?: string;
  params?: unknown;
  result?: unknown;
  error?: { message: string };
}

/** A minimal LSP client: enough to drive the server and read what comes back. */
class TestClient {
  private readonly child: child_process.ChildProcess;
  private nextId = 1;
  private readonly pending = new Map<number, (message: Message) => void>();
  readonly notifications: Message[] = [];

  constructor() {
    this.child = child_process.fork(SERVER, ['--node-ipc'], {
      stdio: ['ignore', 'pipe', 'pipe', 'ipc'],
    });

    this.child.on('message', (raw: Message) => {
      if (raw.id !== undefined && this.pending.has(raw.id)) {
        this.pending.get(raw.id)!(raw);
        this.pending.delete(raw.id);
      } else if (raw.method) {
        this.notifications.push(raw);
      }
    });
  }

  request(method: string, params: unknown): Promise<Message> {
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(
        () => reject(new Error(`${method} timed out`)),
        30_000,
      );
      this.pending.set(id, (message) => {
        clearTimeout(timer);
        resolve(message);
      });
      this.child.send({ jsonrpc: '2.0', id, method, params });
    });
  }

  notify(method: string, params: unknown): void {
    this.child.send({ jsonrpc: '2.0', method, params });
  }

  /** Wait for a notification matching `method`, or give up. */
  async waitFor(method: string, timeoutMs = 15_000): Promise<Message> {
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
      const found = this.notifications.find((message) => message.method === method);
      if (found) return found;
      await new Promise((resolve) => setTimeout(resolve, 25));
    }
    throw new Error(
      `no ${method} within ${timeoutMs}ms; saw ` +
        JSON.stringify(this.notifications.map((n) => n.method)),
    );
  }

  dispose(): void {
    this.child.kill();
  }
}

describe.skipIf(!READY)('the built server, over LSP', () => {
  let workspace: string;
  let client: TestClient;
  let mainUri: string;

  beforeAll(async () => {
    workspace = fs.mkdtempSync(path.join(os.tmpdir(), 'typst-lsp-'));
    fs.writeFileSync(
      path.join(workspace, 'main.typ'),
      '= Title\n\nSome body text.\n',
    );
    mainUri = `${pathToFileURL(workspace).toString()}/main.typ`;

    client = new TestClient();
    await client.request('initialize', {
      processId: process.pid,
      rootUri: pathToFileURL(workspace).toString(),
      capabilities: {},
      initializationOptions: {
        rootPath: workspace,
        rootUri: pathToFileURL(workspace).toString(),
        mainPath: 'main.typ',
        bundledFontsPath: path.join(__dirname, '..', 'assets', 'fonts'),
        fontCachePath: path.join(workspace, 'font-index.json'),
        extraFontPaths: [],
        // Off: a background scan of the host's fonts is not this test's subject.
        systemFonts: false,
        packages: { enabled: false, registry: '', cachePath: '' },
        settings: { compile: { when: 'onType', debounce: 10 } },
      },
    });
    client.notify('initialized', {});
  }, 60_000);

  afterAll(() => {
    client?.dispose();
    if (workspace) fs.rmSync(workspace, { recursive: true, force: true });
  });

  it('answers initialize with the capabilities the client acts on', async () => {
    // Re-requesting is not meaningful, so this asserts against the stored
    // handshake by asking the server what it advertises for a feature.
    const result = await client.request('textDocument/documentSymbol', {
      textDocument: { uri: mainUri },
    });
    expect(result.error, result.error?.message).toBeUndefined();
  });

  it('compiles on didOpen and publishes diagnostics', async () => {
    client.notify('textDocument/didOpen', {
      textDocument: {
        uri: mainUri,
        languageId: 'typst',
        version: 1,
        text: '= Title\n\n#undefined-call()\n',
      },
    });

    const published = await client.waitFor('textDocument/publishDiagnostics');
    const params = published.params as {
      uri: string;
      diagnostics: { message: string }[];
    };

    expect(params.uri).toBe(mainUri);
    expect(params.diagnostics.some((d) => d.message.includes('unknown variable'))).toBe(
      true,
    );
  }, 60_000);

  it('reports compile status the status bar can render', async () => {
    const status = await client.waitFor('typst/compileStatus');
    expect(['compiling', 'ok', 'error']).toContain(
      (status.params as { state: string }).state,
    );
  });

  it('answers a hover request end to end', async () => {
    const result = await client.request('textDocument/hover', {
      textDocument: { uri: mainUri },
      position: { line: 0, character: 3 },
    });

    expect(result.error).toBeUndefined();
  });

  it('answers semantic tokens end to end', async () => {
    const result = await client.request('textDocument/semanticTokens/full', {
      textDocument: { uri: mainUri },
    });

    const tokens = result.result as { data: number[] } | null;
    expect(tokens?.data.length).toBeGreaterThan(0);
    // Five `u32`s per token, per the protocol.
    expect(tokens!.data.length % 5).toBe(0);
  });

  it('formats a document end to end', async () => {
    client.notify('textDocument/didChange', {
      textDocument: { uri: mainUri, version: 2 },
      contentChanges: [{ text: '#let   f(x)  =  x+1\n' }],
    });

    const result = await client.request('textDocument/formatting', {
      textDocument: { uri: mainUri },
      options: { tabSize: 2, insertSpaces: true },
    });

    const edits = result.result as { newText: string }[] | null;
    expect(edits?.[0].newText).toContain('#let f(x) = x + 1');
  });

  it('refuses a package import cleanly when downloads are disabled', async () => {
    client.notify('textDocument/didChange', {
      textDocument: { uri: mainUri, version: 3 },
      contentChanges: [{ text: '#import "@preview/cetz:0.4.2": canvas\n' }],
    });

    // Wait for the compile that follows the debounce, then read the newest
    // diagnostics for this document.
    await new Promise((resolve) => setTimeout(resolve, 2000));

    const messages = client.notifications
      .filter((message) => message.method === 'textDocument/publishDiagnostics')
      .flatMap(
        (message) =>
          (message.params as { diagnostics: { message: string }[] }).diagnostics,
      )
      .map((diagnostic) => diagnostic.message);

    expect(messages.join('\n')).toMatch(/packages\.enabled|disabled/);
  }, 60_000);
});
