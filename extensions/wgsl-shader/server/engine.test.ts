// The WASM boundary itself.
//
// Every feature is tested in Rust, against the same three entry points. What
// is left to go wrong is the crossing: a method name the Rust dispatcher does
// not recognise, a params shape that fails to deserialize, a result that does
// not survive `JSON.parse`. Those are exactly what this exercises, and they
// are invisible to `cargo test`.
//
// Skipped when the artifact has not been built, so `pnpm test` works in a
// checkout without a WASM toolchain.

import * as fs from 'fs';
import * as path from 'path';
import { describe, expect, it } from 'vitest';

const ENTRY = path.join(__dirname, '..', 'wasm', 'wgsl_lsp_wasm.js');
const built = fs.existsSync(ENTRY);

interface ServerInstance {
  capabilities(): Record<string, unknown>;
  onRequest(method: string, params: unknown): unknown;
  onNotification(method: string, params: unknown): void;
  drainEvents(): { method: string; params: unknown }[];
}

interface Wasm {
  ShaderServer: { new (init: unknown): ServerInstance; nagaVersion(): string };
}

function load(): Wasm {
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  return require(ENTRY) as Wasm;
}

/** The LSP position of the character `plus` bytes into `needle`. */
function positionOf(text: string, needle: string, plus = 0): { line: number; character: number } {
  const offset = text.indexOf(needle) + plus;
  const before = text.slice(0, offset);
  return {
    line: before.split('\n').length - 1,
    character: offset - (before.lastIndexOf('\n') + 1),
  };
}

const WGSL = `struct Camera { view: mat4x4f, eye: vec3f }
@group(0) @binding(0) var<uniform> camera: Camera;

@fragment
fn fs_main() -> @location(0) vec4f {
  return vec4f(camera.eye, 1.0);
}
`;

function open(server: ServerInstance, uri: string, languageId: string, text: string): void {
  server.onNotification('textDocument/didOpen', {
    textDocument: { uri, languageId, version: 1, text },
  });
}

describe.skipIf(!built)('the WASM binding', () => {
  it('reports the naga version it validates with', () => {
    expect(load().ShaderServer.nagaVersion()).toMatch(/^\d+\.\d+\.\d+$/);
  });

  it('advertises the capabilities the client needs', () => {
    const server = new (load().ShaderServer)({ settings: {} });
    const capabilities = server.capabilities();
    for (const capability of [
      'textDocumentSync',
      'completionProvider',
      'hoverProvider',
      'definitionProvider',
      'referencesProvider',
      'renameProvider',
      'documentSymbolProvider',
      'workspaceSymbolProvider',
      'foldingRangeProvider',
      'signatureHelpProvider',
      'codeActionProvider',
      'semanticTokensProvider',
    ]) {
      expect(capabilities, capability).toHaveProperty(capability);
    }
    // Off by default, so not advertised.
    expect(capabilities).not.toHaveProperty('inlayHintProvider');
    expect(capabilities).not.toHaveProperty('documentFormattingProvider');
  });

  it('round-trips every request `server/main.ts` forwards', () => {
    const server = new (load().ShaderServer)({ settings: {} });
    const uri = 'file:///t.wgsl';
    open(server, uri, 'wgsl', WGSL);

    const position = positionOf(WGSL, 'camera.eye', 8);
    const textDocument = { textDocument: { uri } };

    // No throw is the assertion for most of these: a params shape the Rust
    // side cannot deserialize surfaces here and nowhere else.
    expect(() => {
      server.onRequest('textDocument/completion', { ...textDocument, position });
      server.onRequest('textDocument/hover', { ...textDocument, position });
      server.onRequest('textDocument/definition', { ...textDocument, position });
      server.onRequest('textDocument/documentHighlight', { ...textDocument, position });
      server.onRequest('textDocument/signatureHelp', { ...textDocument, position });
      server.onRequest('textDocument/prepareRename', { ...textDocument, position });
      server.onRequest('textDocument/references', {
        ...textDocument,
        position,
        context: { includeDeclaration: true },
      });
      server.onRequest('textDocument/rename', {
        ...textDocument,
        position,
        newName: 'nose',
      });
      server.onRequest('textDocument/documentSymbol', textDocument);
      server.onRequest('workspace/symbol', { query: 'cam' });
      server.onRequest('textDocument/foldingRange', textDocument);
      server.onRequest('textDocument/codeAction', {
        ...textDocument,
        range: { start: position, end: position },
        context: { diagnostics: [] },
      });
      server.onRequest('textDocument/semanticTokens/full', textDocument);
      server.onRequest('textDocument/inlayHint', {
        ...textDocument,
        range: { start: { line: 0, character: 0 }, end: { line: 99, character: 0 } },
      });
      server.onRequest('wgsl/shaderInfo', textDocument);
    }).not.toThrow();
  });

  it('rejects a method it does not implement', () => {
    const server = new (load().ShaderServer)({ settings: {} });
    expect(() => server.onRequest('textDocument/telepathy', {})).toThrow();
  });

  it('publishes diagnostics as drained events', () => {
    const server = new (load().ShaderServer)({ settings: {} });
    open(server, 'file:///bad.wgsl', 'wgsl', 'fn main() { let x = nope(); }\n');

    const events = server.drainEvents();
    expect(events).toHaveLength(1);
    expect(events[0]!.method).toBe('textDocument/publishDiagnostics');
    const params = events[0]!.params as { diagnostics: unknown[] };
    expect(params.diagnostics.length).toBeGreaterThan(0);
  });

  /// The behaviour the whole design turns on: a `.` makes the document
  /// unparseable, and completion has to answer anyway.
  it('completes members after a dot the user has just typed', () => {
    const server = new (load().ShaderServer)({ settings: {} });
    const uri = 'file:///t.wgsl';
    open(server, uri, 'wgsl', WGSL);

    const typed = WGSL.replace('camera.eye', 'camera.');
    server.onNotification('textDocument/didChange', {
      textDocument: { uri, version: 2 },
      contentChanges: [{ text: typed }],
    });

    const items = server.onRequest('textDocument/completion', {
      textDocument: { uri },
      position: positionOf(typed, 'camera.', 7),
    }) as { label: string }[];
    expect(items.map((item) => item.label)).toEqual(['view', 'eye']);
  });

  it('serves GLSL as well as WGSL, stage and all', () => {
    const server = new (load().ShaderServer)({ settings: {} });
    const uri = 'file:///t.frag';
    open(
      server,
      uri,
      'glsl',
      '#version 450\nlayout(location = 0) out vec4 colour;\nvoid main() { colour = vec4(1.0); }\n',
    );

    const info = server.onRequest('wgsl/shaderInfo', { textDocument: { uri } }) as {
      language: string;
      stage: string;
      ok: boolean;
    };
    expect(info.language).toBe('glsl');
    expect(info.stage).toBe('fragment');
    expect(info.ok).toBe(true);
  });

  it('takes settings at construction and again over didChangeConfiguration', () => {
    const off = new (load().ShaderServer)({
      settings: { wgsl: { completion: { enabled: false } }, glsl: {} },
    });
    expect(off.capabilities()).toHaveProperty('completionProvider');

    const server = new (load().ShaderServer)({ settings: {} });
    open(server, 'file:///t.wgsl', 'wgsl', WGSL);
    expect(() =>
      server.onNotification('workspace/didChangeConfiguration', {
        settings: { wgsl: { validate: { onType: true } }, glsl: {} },
      }),
    ).not.toThrow();
  });
});
