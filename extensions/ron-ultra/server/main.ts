import * as fs from 'fs';
import * as path from 'path';
import {
  createConnection,
  ProposedFeatures,
  type Connection,
  type InitializeParams,
  type InitializeResult,
} from 'vscode-languageserver/node';
import { host, type HostServices } from './host.js';

/**
 * The RON language server.
 *
 * Runs as a child process of the extension host, so the WASM heap and a
 * panic stay out of it. Everything here is glue: the features live in Rust
 * (`crates/ron-ultra/ron-lsp-core`) and are tested there against the same
 * three entry points this file calls.
 *
 * The server reads files it was not sent — the `Cargo.toml` and `.rs` files
 * behind a `@[crate::config::GameConfig]` annotation — through `readFile`,
 * which it calls synchronously in the middle of a request.
 */

/** The shape `wasm/ron_lsp_wasm.js` exposes. */
interface WasmModule {
  RonServer: {
    new (host: HostServices, init: unknown): RonServerInstance;
    requests(): string[];
    notifications(): string[];
  };
}

interface RonServerInstance {
  capabilities(): InitializeResult['capabilities'];
  onRequest(method: string, params: unknown): unknown;
  onNotification(method: string, params: unknown): void;
  drainEvents(): { method: string; params: unknown }[];
}

/** What the extension host passes at startup. */
interface InitOptions {
  /** The `ronUltra.*` settings. */
  settings: Record<string, unknown>;
}

const connection: Connection = createConnection(ProposedFeatures.all);
let server: RonServerInstance | null = null;

connection.onInitialize((params: InitializeParams): InitializeResult => {
  const options = (params.initializationOptions ?? {}) as Partial<InitOptions>;
  let wasm: WasmModule;
  try {
    wasm = loadWasm();
  } catch (error) {
    // Come up answering nothing rather than crash: the log says what to run.
    connection.console.error(
      'RON engine not built. Run `pnpm run build:wasm` in extensions/ron-ultra.\n' + String(error),
    );
    return { capabilities: {} };
  }
  server = new wasm.RonServer(host, { settings: options.settings ?? {} });
  register(wasm);
  return { capabilities: server.capabilities() };
});

/** Route every method the Rust side handles to it. */
function register(wasm: WasmModule): void {
  for (const method of wasm.RonServer.requests()) {
    connection.onRequest(method, (params: unknown) => {
      if (!server) return null;
      const result = server.onRequest(method, params);
      flush();
      return result;
    });
  }
  for (const method of wasm.RonServer.notifications()) {
    connection.onNotification(method, (params: unknown) => {
      if (!server) return;
      server.onNotification(method, params);
      flush();
    });
  }
}

/**
 * Send what the last call queued — after its response, so Rust never
 * re-enters the host mid-handler.
 */
function flush(): void {
  if (!server) return;
  for (const event of server.drainEvents()) {
    void connection.sendNotification(event.method, event.params);
  }
}

/** A computed `require` so the bundler leaves the WASM glue on disk. */
function loadWasm(): WasmModule {
  const entry = path.join(__dirname, '..', 'wasm', 'ron_lsp_wasm.js');
  if (!fs.existsSync(entry)) {
    throw new Error(`${entry} does not exist`);
  }
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  return require(entry) as WasmModule;
}

connection.listen();
