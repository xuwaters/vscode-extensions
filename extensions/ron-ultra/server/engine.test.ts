// The WASM boundary itself.
//
// Every feature is tested in Rust against the same three entry points. What
// is left to go wrong is the crossing: a method the dispatcher does not
// recognise, params that fail to deserialize, a host callback that cannot be
// called synchronously, a result that does not survive `JSON.parse`.
//
// Skipped when the artifact has not been built, so `pnpm test` works in a
// checkout without a WASM toolchain.

import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import { pathToFileURL } from 'url';
import { afterAll, describe, expect, it } from 'vitest';
import { host } from './host.js';

const ENTRY = path.join(__dirname, '..', 'wasm', 'ron_lsp_wasm.js');
const built = fs.existsSync(ENTRY);

interface ServerInstance {
  capabilities(): Record<string, unknown>;
  onRequest(method: string, params: unknown): unknown;
  onNotification(method: string, params: unknown): void;
  drainEvents(): { method: string; params: { diagnostics?: { code: string }[] } }[];
}

interface Wasm {
  RonServer: {
    new (host: unknown, init: unknown): ServerInstance;
    requests(): string[];
    notifications(): string[];
  };
}

function load(): Wasm {
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  return require(ENTRY) as Wasm;
}

// A tiny crate on disk, read through the real host callback.
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'ron-ultra-'));
fs.mkdirSync(path.join(root, 'src'));
fs.mkdirSync(path.join(root, 'assets'));
fs.writeFileSync(path.join(root, 'Cargo.toml'), '[package]\nname = "game"\n');
fs.writeFileSync(path.join(root, 'src', 'lib.rs'), 'pub mod config;\n');
fs.writeFileSync(
  path.join(root, 'src', 'config.rs'),
  'pub struct GameConfig {\n    pub fullscreen: bool,\n    pub mode: Mode,\n}\npub enum Mode { Windowed, Borderless }\n',
);
const ronUri = pathToFileURL(path.join(root, 'assets', 'config.ron')).toString();

afterAll(() => fs.rmSync(root, { recursive: true, force: true }));

function open(server: ServerInstance, text: string): void {
  server.onNotification('textDocument/didOpen', {
    textDocument: { uri: ronUri, languageId: 'ron', version: 1, text },
  });
}

describe('the host', () => {
  it('reads files by URI and answers null for anything missing', () => {
    expect(host.readFile(pathToFileURL(path.join(root, 'Cargo.toml')).toString())).toContain('game');
    expect(host.readFile(pathToFileURL(path.join(root, 'nope.rs')).toString())).toBeNull();
    expect(host.readFile(pathToFileURL(path.join(root, 'src')).toString())).toBeNull();
    expect(host.readFile('untitled:Untitled-1')).toBeNull();
  });
});

describe.skipIf(!built)('the WASM binding', () => {
  it('advertises its methods and capabilities', () => {
    const wasm = load();
    expect(wasm.RonServer.requests()).toContain('textDocument/completion');
    expect(wasm.RonServer.notifications()).toContain('workspace/didChangeWatchedFiles');
    const caps = new wasm.RonServer(host, { settings: {} }).capabilities();
    for (const c of ['textDocumentSync', 'completionProvider', 'hoverProvider', 'definitionProvider']) {
      expect(caps).toHaveProperty(c);
    }
  });

  it('checks a file against Rust read through the host, synchronously', () => {
    const server = new (load().RonServer)(host, { settings: {} });
    open(server, '/* @[crate::config::GameConfig] */\nGameConfig(fullscreen: 1, mode: Tiled)\n');
    const publish = server.drainEvents().find((e) => e.method === 'textDocument/publishDiagnostics');
    expect(publish?.params.diagnostics?.map((d) => d.code)).toEqual(['RON013', 'RON016']);
  });

  it('answers completion and definition with LSP shapes', () => {
    const server = new (load().RonServer)(host, { settings: {} });
    open(server, '/* @[crate::config::GameConfig] */\nGameConfig(fullscreen: true, mode: )\n');
    const items = server.onRequest('textDocument/completion', {
      textDocument: { uri: ronUri },
      position: { line: 1, character: 35 },
    }) as { label: string }[];
    expect(items.map((i) => i.label)).toEqual(['Windowed', 'Borderless']);

    const def = server.onRequest('textDocument/definition', {
      textDocument: { uri: ronUri },
      position: { line: 1, character: 13 },
    }) as { uri: string; range: { start: { line: number } } };
    expect(def.uri).toBe(pathToFileURL(path.join(root, 'src', 'config.rs')).toString());
    expect(def.range.start.line).toBe(1);
  });

  it('rejects unknown methods with a JSON-RPC error', () => {
    const server = new (load().RonServer)(host, { settings: {} });
    expect(() => server.onRequest('nope', {})).toThrow();
  });
});
