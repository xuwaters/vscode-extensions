import * as fs from 'fs';
import * as path from 'path';
import {
  createConnection,
  ProposedFeatures,
  type Connection,
  type InitializeParams,
  type InitializeResult,
} from 'vscode-languageserver/node';

/**
 * The WGSL / GLSL language server.
 *
 * Runs as a child process of the extension host. A shader compiler is not the
 * kind of thing to run in-process: naga panicking on a malformed module would
 * take down every extension in the window, and the WASM heap is never returned
 * to the OS once grown.
 *
 * Everything here is glue. The interesting parts are in Rust — in
 * `crates/wgsl/wgsl-lsp-core` — and are tested there, against the same three
 * entry points this file calls.
 */

/** The shape `wasm/wgsl_lsp_wasm.js` exposes. */
interface WasmModule {
  ShaderServer: {
    new (init: unknown): ShaderServerInstance;
    heapBytes(): number;
    nagaVersion(): string;
  };
}

interface ShaderServerInstance {
  capabilities(): InitializeResult['capabilities'];
  onRequest(method: string, params: unknown): unknown;
  onNotification(method: string, params: unknown): void;
  drainEvents(): { method: string; params: unknown }[];
}

/** What the extension host passes at startup. */
interface InitOptions {
  /** The `wgsl.*` and `glsl.*` settings the Rust side reads. */
  settings: Record<string, unknown>;
}

const connection: Connection = createConnection(ProposedFeatures.all);

/** Requests the Rust side answers. Anything else gets a method-not-found. */
const REQUESTS = [
  'textDocument/completion',
  'textDocument/hover',
  'textDocument/definition',
  'textDocument/references',
  'textDocument/documentHighlight',
  'textDocument/signatureHelp',
  'textDocument/documentSymbol',
  'workspace/symbol',
  'textDocument/foldingRange',
  'textDocument/prepareRename',
  'textDocument/rename',
  'textDocument/codeAction',
  'textDocument/formatting',
  'textDocument/rangeFormatting',
  'textDocument/semanticTokens/full',
  'textDocument/semanticTokens/full/delta',
  'textDocument/inlayHint',
  // Ours: GLSL has no in-language way to say which stage a file is, so the
  // status bar has to ask what the server decided.
  'wgsl/shaderInfo',
] as const;

/** Notifications the Rust side handles. */
const NOTIFICATIONS = [
  'textDocument/didOpen',
  'textDocument/didChange',
  'textDocument/didSave',
  'textDocument/didClose',
  'workspace/didChangeConfiguration',
  // The server has no filesystem — it runs inside WASM — so the host walks the
  // workspace and pushes what it finds.
  'wgsl/workspaceFiles',
] as const;

let server: ShaderServerInstance | null = null;

connection.onInitialize((params: InitializeParams): InitializeResult => {
  const options = (params.initializationOptions ?? {}) as Partial<InitOptions>;

  let wasm: WasmModule;
  try {
    wasm = loadWasm();
  } catch (error) {
    // Graceful degradation rather than a crashed process: the server comes up,
    // says exactly what to run, and answers nothing.
    connection.console.error(
      'Shader engine not built. Run `pnpm run build:wasm` in extensions/wgsl-shader.\n' +
        String(error),
    );
    connection.sendNotification('wgsl/engineMissing', { command: 'pnpm run build:wasm' });
    return { capabilities: {} };
  }

  server = new wasm.ShaderServer({ settings: options.settings ?? {} });
  connection.console.log(`WGSL/GLSL server ready, validating with naga ${wasm.ShaderServer.nagaVersion()}`);

  return { capabilities: server.capabilities() };
});

for (const method of REQUESTS) {
  connection.onRequest(method, (params: unknown) => {
    if (!server) return null;
    const result = server.onRequest(method, params);
    flush();
    return result;
  });
}

for (const method of NOTIFICATIONS) {
  connection.onNotification(method, (params: unknown) => {
    if (!server) return;
    server.onNotification(method, params);
    flush();
  });
}

/**
 * Send everything the last call queued.
 *
 * Drained *after* the response is written, so Rust never re-enters the host
 * mid-handler — the one ordering rule the whole boundary rests on.
 */
function flush(): void {
  if (!server) return;
  for (const event of server.drainEvents()) {
    void connection.sendNotification(event.method, event.params);
  }
}

/**
 * Load the WASM glue with a computed `require`, resolved relative to the
 * extension root.
 *
 * Computed so the bundler leaves it alone: the artifact is loaded from disk at
 * runtime, not inlined into `dist/server.js`.
 */
function loadWasm(): WasmModule {
  const entry = path.join(__dirname, '..', 'wasm', 'wgsl_lsp_wasm.js');
  if (!fs.existsSync(entry)) {
    throw new Error(`${entry} does not exist`);
  }
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  return require(entry) as WasmModule;
}

connection.listen();
