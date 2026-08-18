import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import { pathToFileURL } from 'url';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { FontIndex } from './fonts.js';
import { listDir, readFile, type Roots } from './vfs.js';

/**
 * The binding layer, driven through the real artifact.
 *
 * Everything below the WASM boundary is covered by `cargo test`; what this
 * covers is the boundary itself — that synchronous host callbacks work from
 * inside a compile, that the JSON shapes line up, and that the whole stack
 * produces a document from a file on disk.
 *
 * Skipped when `wasm/` has not been built, so `pnpm test` works on a checkout
 * that has not run `pnpm run build:wasm`.
 */

const WASM = path.join(__dirname, '..', 'wasm', 'typst_lsp_wasm.js');
const BUILT = fs.existsSync(WASM);

interface TypstServerInstance {
  capabilities(): Record<string, unknown>;
  onRequest(method: string, params: unknown): unknown;
  onNotification(method: string, params: unknown): void;
  drainEvents(): { method: string; params: unknown }[];
  setFontFaces(faces: { info: unknown; index: number }[]): void;
}

interface WasmModule {
  TypstServer: {
    new (host: unknown, init: unknown): TypstServerInstance;
    indexFont(data: Uint8Array): { info: unknown; index: number }[];
    heapBytes(): number;
    typstVersion(): string;
  };
}

describe.skipIf(!BUILT)('the WASM engine', () => {
  let workspace: string;
  let wasm: WasmModule;
  let server: TypstServerInstance;
  let mainUri: string;
  let reads: string[];

  beforeAll(() => {
    workspace = fs.mkdtempSync(path.join(os.tmpdir(), 'typst-ultra-'));
    fs.writeFileSync(
      path.join(workspace, 'main.typ'),
      '#import "helper.typ": greet\n\n= Title\n\n#greet("world")\n',
    );
    fs.writeFileSync(
      path.join(workspace, 'helper.typ'),
      '#let greet(name) = [Hello, #name!]\n',
    );

    // eslint-disable-next-line @typescript-eslint/no-require-imports
    wasm = require(WASM) as WasmModule;

    const roots: Roots = { project: workspace, packageCache: '' };
    const fonts = new FontIndex('', (data) => wasm.TypstServer.indexFont(data));
    fonts.addDirectories([path.join(__dirname, '..', 'assets', 'fonts')]);

    reads = [];
    const host = {
      readFile: (root: string, vpath: string) => {
        reads.push(vpath);
        return readFile(roots, root, vpath);
      },
      listDir: (root: string, vpath: string) => listDir(roots, root, vpath),
      fontData: (face: number) => fonts.data(face),
      resolvePackage: () => 'failed:packages are disabled in this test',
      now: () => Date.UTC(2026, 7, 17, 12, 0, 0),
      timezoneOffsetMinutes: () => 0,
    };

    mainUri = `${pathToFileURL(workspace).toString()}/main.typ`;
    server = new wasm.TypstServer(host, {
      rootUri: pathToFileURL(workspace).toString(),
      mainPath: 'main.typ',
      settings: {},
      fontFaces: fonts.descriptors,
      packages: [],
    });
  });

  afterAll(() => {
    if (workspace) fs.rmSync(workspace, { recursive: true, force: true });
  });

  it('reports the typst version it was built against', () => {
    expect(wasm.TypstServer.typstVersion()).toBe('0.15.1');
  });

  it('advertises the capabilities the extension relies on', () => {
    const capabilities = server.capabilities();
    expect(capabilities.hoverProvider).toBeTruthy();
    expect(capabilities.definitionProvider).toBeTruthy();
    expect(capabilities.completionProvider).toBeTruthy();
    expect(capabilities.semanticTokensProvider).toBeTruthy();
    expect(capabilities.documentFormattingProvider).toBeTruthy();
  });

  it('indexes the bundled fonts', () => {
    const fonts = new FontIndex('', (data) => wasm.TypstServer.indexFont(data));
    const stats = fonts.addDirectories([
      path.join(__dirname, '..', 'assets', 'fonts'),
    ]);
    expect(stats.faces).toBeGreaterThanOrEqual(17);
  });

  it('compiles a multi-file document, reading its imports through the host', () => {
    server.onNotification('textDocument/didOpen', {
      textDocument: {
        uri: mainUri,
        languageId: 'typst',
        version: 1,
        text: fs.readFileSync(path.join(workspace, 'main.typ'), 'utf8'),
      },
    });

    reads.length = 0;
    server.onNotification('typst/compile', { uri: mainUri });
    const events = server.drainEvents();

    // The synchronous callback fired mid-compile — the property the whole VFS
    // design rests on.
    expect(reads).toContain('/helper.typ');

    const status = events.filter((event) => event.method === 'typst/compileStatus');
    const last = status.at(-1)?.params as { state: string; pageCount: number };
    expect(last.state).toBe('ok');
    expect(last.pageCount).toBe(1);

    const diagnostics = events.filter(
      (event) => event.method === 'textDocument/publishDiagnostics',
    );
    for (const event of diagnostics) {
      expect((event.params as { diagnostics: unknown[] }).diagnostics).toEqual([]);
    }
  });

  it('answers a hover from the syntax tree', () => {
    const result = server.onRequest('textDocument/hover', {
      textDocument: { uri: mainUri },
      position: { line: 4, character: 2 },
    }) as { contents?: { value?: string } } | null;

    expect(result?.contents?.value).toBeTruthy();
  });

  it('renders a page to SVG and then ships nothing for an unchanged page', () => {
    const first = server.onRequest('typst/renderPages', {
      uri: mainUri,
      pages: [0],
      knownHashes: {},
    }) as { patches: { op: string; hash?: string; format?: string; content?: string }[] };

    expect(first.patches[0].op).toBe('replace');
    expect(first.patches[0].format).toBe('svg');
    expect(first.patches[0].content).toMatch(/^<svg/);

    const second = server.onRequest('typst/renderPages', {
      uri: mainUri,
      pages: [0],
      knownHashes: { 0: first.patches[0].hash },
    }) as { patches: { op: string }[] };

    expect(second.patches[0].op).toBe('unchanged');
  });

  it('exports a PDF from the last good document', () => {
    const result = server.onRequest('typst/export', { format: 'pdf' }) as {
      files: string[];
      extension: string;
    };

    expect(result.extension).toBe('pdf');
    const bytes = Buffer.from(result.files[0], 'base64');
    expect(bytes.subarray(0, 5).toString('latin1')).toBe('%PDF-');
  });

  it('reports diagnostics for a broken edit and clears them when fixed', () => {
    server.onNotification('textDocument/didChange', {
      textDocument: { uri: mainUri, version: 2 },
      contentChanges: [{ text: '= Title\n\n#undefined-call()\n' }],
    });
    server.onNotification('typst/compile', { uri: mainUri });

    const broken = server
      .drainEvents()
      .filter((event) => event.method === 'textDocument/publishDiagnostics')
      .flatMap(
        (event) => (event.params as { diagnostics: { message: string }[] }).diagnostics,
      );
    expect(broken.some((d) => d.message.includes('unknown variable'))).toBe(true);

    server.onNotification('textDocument/didChange', {
      textDocument: { uri: mainUri, version: 3 },
      contentChanges: [{ text: '= Title\n\nFixed.\n' }],
    });
    server.onNotification('typst/compile', { uri: mainUri });

    const fixed = server
      .drainEvents()
      .filter((event) => event.method === 'textDocument/publishDiagnostics')
      .flatMap((event) => (event.params as { diagnostics: unknown[] }).diagnostics);
    expect(fixed).toEqual([]);
  });

  it('refuses an import from a package it cannot resolve, with a reason', () => {
    server.onNotification('textDocument/didChange', {
      textDocument: { uri: mainUri, version: 4 },
      contentChanges: [{ text: '#import "@preview/cetz:0.4.2": canvas\n' }],
    });
    server.onNotification('typst/compile', { uri: mainUri });

    const messages = server
      .drainEvents()
      .filter((event) => event.method === 'textDocument/publishDiagnostics')
      .flatMap(
        (event) => (event.params as { diagnostics: { message: string }[] }).diagnostics,
      )
      .map((diagnostic) => diagnostic.message);

    expect(messages.join('\n')).toContain('packages are disabled in this test');
  });

  it('reports a heap size the watchdog can act on', () => {
    const bytes = wasm.TypstServer.heapBytes();
    expect(bytes).toBeGreaterThan(1_000_000);
  });

  // The other half of a paper: a `.bib` the compiler reads and the editor edits.
  it('speaks BibTeX for a bibliography, and cites into it', () => {
    const bib = '@article{knuth1984,\n  author = {Knuth, Donald E.},\n  '
      + 'title = {Literate Programming},\n  journal = {The Computer Journal},\n  '
      + 'year = {1984},\n}\n';
    const bibUri = `${pathToFileURL(workspace).toString()}/refs.bib`;
    fs.writeFileSync(path.join(workspace, 'refs.bib'), bib);

    server.onNotification('textDocument/didOpen', {
      textDocument: { uri: bibUri, languageId: 'bibtex', version: 1, text: bib },
    });
    server.onNotification('textDocument/didChange', {
      textDocument: { uri: mainUri, version: 5 },
      contentChanges: [
        { text: '#bibliography("refs.bib")\n\nSee @knuth1984.\n' },
      ],
    });

    // The host debounces off the last edited file, which is the bibliography.
    // Compiling *it* would hand BibTeX to the typst compiler.
    server.drainEvents();
    server.onNotification('typst/compile', { uri: bibUri });
    const events = server.drainEvents();

    const status = events.filter((event) => event.method === 'typst/compileStatus');
    const last = status.at(-1)?.params as { state: string; pageCount: number };
    expect(last.state).toBe('ok');
    expect(last.pageCount).toBe(1);

    const symbols = server.onRequest('textDocument/documentSymbol', {
      textDocument: { uri: bibUri },
    }) as { name: string }[];
    expect(symbols.map((symbol) => symbol.name)).toEqual(['knuth1984']);

    const definition = server.onRequest('textDocument/definition', {
      textDocument: { uri: mainUri },
      position: { line: 2, character: 8 },
    }) as { uri: string; range: { start: { line: number } } };
    expect(definition.uri).toBe(bibUri);
    expect(definition.range.start.line).toBe(0);

    // Break the bibliography: its problems come from the parser, on the edit.
    server.onNotification('textDocument/didChange', {
      textDocument: { uri: bibUri, version: 2 },
      contentChanges: [{ text: `${bib}\n@misc{knuth1984, title = {Again}}\n` }],
    });
    const problems = server
      .drainEvents()
      .filter((event) => event.method === 'textDocument/publishDiagnostics')
      .filter((event) => (event.params as { uri: string }).uri === bibUri)
      .flatMap(
        (event) =>
          (event.params as { diagnostics: { message: string; source: string }[] })
            .diagnostics,
      );

    expect(problems.some((problem) => problem.message.includes('duplicate'))).toBe(true);
    expect(problems.every((problem) => problem.source === 'bibtex')).toBe(true);
  });
});
