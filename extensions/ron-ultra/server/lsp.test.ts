// The bundled server as the editor starts it: a child process speaking
// JSON-RPC. This is what catches a method the glue forgot to register, an
// `initialize` that never answers, or a shutdown handshake the glue broke.
//
// Skipped until both `dist/server.js` and the WASM artifact are built.

import { spawn, type ChildProcessWithoutNullStreams } from 'child_process';
import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import { pathToFileURL } from 'url';
import { afterAll, describe, expect, it } from 'vitest';

const EXT = path.join(__dirname, '..');
const SERVER = path.join(EXT, 'dist', 'server.js');
const built = fs.existsSync(SERVER) && fs.existsSync(path.join(EXT, 'wasm', 'ron_lsp_wasm.js'));

interface Message {
  id?: number;
  method?: string;
  params?: Record<string, unknown>;
  result?: unknown;
}

/** A minimal LSP client over stdio with Content-Length framing. */
class Peer {
  private buffer = Buffer.alloc(0);
  private nextId = 1;
  private readonly waiting: { match: (m: Message) => boolean; resolve: (m: Message) => void }[] = [];
  private readonly seen: Message[] = [];

  constructor(private readonly child: ChildProcessWithoutNullStreams) {
    child.stdout.on('data', (chunk: Buffer) => this.receive(chunk));
  }

  private receive(chunk: Buffer): void {
    this.buffer = Buffer.concat([this.buffer, chunk]);
    for (;;) {
      const headerEnd = this.buffer.indexOf('\r\n\r\n');
      if (headerEnd < 0) return;
      const length = Number(/Content-Length: (\d+)/i.exec(this.buffer.subarray(0, headerEnd).toString())?.[1]);
      const start = headerEnd + 4;
      if (this.buffer.length < start + length) return;
      const message = JSON.parse(this.buffer.subarray(start, start + length).toString()) as Message;
      this.buffer = this.buffer.subarray(start + length);
      const i = this.waiting.findIndex((w) => w.match(message));
      if (i >= 0) this.waiting.splice(i, 1)[0].resolve(message);
      else this.seen.push(message);
    }
  }

  private send(message: object): void {
    const body = JSON.stringify({ jsonrpc: '2.0', ...message });
    this.child.stdin.write(`Content-Length: ${Buffer.byteLength(body)}\r\n\r\n${body}`);
  }

  wait(match: (m: Message) => boolean): Promise<Message> {
    const i = this.seen.findIndex(match);
    if (i >= 0) return Promise.resolve(this.seen.splice(i, 1)[0]);
    return new Promise((resolve) => this.waiting.push({ match, resolve }));
  }

  async request(method: string, params: unknown): Promise<unknown> {
    const id = this.nextId++;
    this.send({ id, method, params });
    return (await this.wait((m) => m.id === id && m.method === undefined)).result;
  }

  notify(method: string, params: unknown): void {
    this.send({ method, params });
  }
}

const root = fs.mkdtempSync(path.join(os.tmpdir(), 'ron-ultra-lsp-'));
afterAll(() => fs.rmSync(root, { recursive: true, force: true }));

describe.skipIf(!built)('the bundled server', () => {
  it('initializes, publishes schema diagnostics, completes, and shuts down', async () => {
    fs.mkdirSync(path.join(root, 'src'));
    fs.writeFileSync(path.join(root, 'Cargo.toml'), '[package]\nname = "game"\n');
    fs.writeFileSync(path.join(root, 'src', 'lib.rs'), 'pub struct Config { pub level: Level }\npub enum Level { Easy, Hard }\n');
    const uri = pathToFileURL(path.join(root, 'level.ron')).toString();

    const child = spawn(process.execPath, [SERVER, '--stdio'], { stdio: 'pipe' });
    const peer = new Peer(child);
    try {
      const init = (await peer.request('initialize', {
        processId: process.pid,
        rootUri: null,
        capabilities: {},
        initializationOptions: { settings: {} },
      })) as { capabilities: Record<string, unknown> };
      expect(init.capabilities).toHaveProperty('completionProvider');
      peer.notify('initialized', {});

      peer.notify('textDocument/didOpen', {
        textDocument: { uri, languageId: 'ron', version: 1, text: '// @[crate::Config]\nConfig(level: Medium)\n' },
      });
      const publish = await peer.wait((m) => m.method === 'textDocument/publishDiagnostics');
      const codes = (publish.params?.diagnostics as { code: string }[]).map((d) => d.code);
      expect(codes).toEqual(['RON016']);

      const items = (await peer.request('textDocument/completion', {
        textDocument: { uri },
        position: { line: 1, character: 14 },
      })) as { label: string }[];
      expect(items.map((i) => i.label)).toEqual(['Easy', 'Hard']);

      expect(await peer.request('shutdown', null)).toBeNull();
      const exited = new Promise<number | null>((resolve) => child.on('exit', resolve));
      peer.notify('exit', null);
      expect(await exited).toBe(0);
    } finally {
      child.kill();
    }
  });
});
