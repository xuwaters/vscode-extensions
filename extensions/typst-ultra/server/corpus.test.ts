import * as fs from 'fs';
import * as path from 'path';
import { pathToFileURL } from 'url';
import { describe, expect, it } from 'vitest';
import { FontIndex } from './fonts.js';
import { listDir, readFile, type Roots } from './vfs.js';

/**
 * P4-08: the real-world corpus, through the real WASM artifact.
 *
 * `cargo run -p typst-session --example corpus` measures the same documents
 * natively, which is useful for comparing *documents* to each other. This
 * measures them where the extension actually runs — inside WASM, in a Node
 * process — because that is what every number in the RFC is a claim about.
 *
 * Both matter, and they are not interchangeable: native has multi-threaded
 * `rayon` layout that WASM does not.
 *
 * The documents live with the engine, in
 * `crates/typst/typst-session/examples/corpus`, so there is one corpus rather
 * than two that drift apart.
 */

const WASM = path.join(__dirname, '..', 'wasm', 'typst_lsp_wasm.js');
const CORPUS = path.join(
  __dirname,
  '..',
  '..',
  '..',
  'crates',
  'typst',
  'typst-session',
  'examples',
  'corpus',
);
const READY = fs.existsSync(WASM) && fs.existsSync(CORPUS);

/** The targets proposal.md §8 sets. */
const TARGETS = {
  coldCompileMs: 1500,
  keystrokeMs: 120,
  heapMb: 400,
};

interface Result {
  name: string;
  pages: number;
  coldMs: number;
  keystrokeMs: number;
  p95Ms: number;
  largestPageKb: number;
  heapMb: number;
}

function median(values: number[]): number {
  return percentile(values, 50);
}

function percentile(values: number[], p: number): number {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.round((p / 100) * (sorted.length - 1))];
}

describe.skipIf(!READY)('the real-world corpus, under WASM', () => {
  it('meets the performance targets on documents that are not synthetic', () => {
    // eslint-disable-next-line @typescript-eslint/no-require-imports
    const wasm = require(WASM);

    const results: Result[] = [];
    for (const [name, file] of [
      ['paper', 'paper.typ'],
      ['book', 'book.typ'],
      ['slides', 'slides.typ'],
      ['graphics', 'graphics.typ'],
    ]) {
      results.push(measure(wasm, name, file));
    }

    console.log('\n| Document | Pages | Cold | Keystroke | p95 | Largest page | Heap |');
    console.log('| --- | --- | --- | --- | --- | --- | --- |');
    for (const r of results) {
      console.log(
        `| ${r.name} | ${r.pages} | ${r.coldMs.toFixed(0)} ms | ` +
          `${r.keystrokeMs.toFixed(1)} ms | ${r.p95Ms.toFixed(1)} ms | ` +
          `${r.largestPageKb.toFixed(0)} KB | ${r.heapMb.toFixed(0)} MB |`,
      );
    }
    console.log();

    for (const r of results) {
      expect(r.pages, `${r.name} produced no pages`).toBeGreaterThan(0);
      expect(r.coldMs, `${r.name} cold compile`).toBeLessThan(TARGETS.coldCompileMs);
      expect(r.keystrokeMs, `${r.name} keystroke`).toBeLessThan(TARGETS.keystrokeMs);
    }

    // Heap is process-wide and cumulative across fixtures, so the ceiling is
    // checked once at the end rather than per document.
    const peak = Math.max(...results.map((r) => r.heapMb));
    expect(peak, 'peak WASM heap across the whole corpus').toBeLessThan(TARGETS.heapMb);
  }, 300_000);
});

function measure(
  wasm: {
    TypstServer: {
      new (host: unknown, init: unknown): {
        onRequest(method: string, params: unknown): unknown;
        onNotification(method: string, params: unknown): void;
        drainEvents(): unknown[];
      };
      indexFont(data: Uint8Array): { info: unknown; index: number }[];
      heapBytes(): number;
    };
  },
  name: string,
  file: string,
): Result {
  const roots: Roots = { project: CORPUS, packageCache: '' };
  const fonts = new FontIndex('', (data) => wasm.TypstServer.indexFont(data));
  fonts.addDirectories([path.join(__dirname, '..', 'assets', 'fonts')]);

  const server = new wasm.TypstServer(
    {
      readFile: (root: string, vpath: string) => readFile(roots, root, vpath),
      listDir: (root: string, vpath: string) => listDir(roots, root, vpath),
      fontData: (face: number) => fonts.data(face),
      resolvePackage: () => 'failed:the corpus uses no packages',
      now: () => Date.UTC(2026, 7, 17),
      timezoneOffsetMinutes: () => 0,
    },
    {
      rootUri: pathToFileURL(CORPUS).toString(),
      mainPath: file,
      settings: {},
      fontFaces: fonts.descriptors,
      packages: [],
    },
  );

  const uri = `${pathToFileURL(CORPUS).toString()}/${file}`;
  let text = fs.readFileSync(path.join(CORPUS, file), 'utf8');

  server.onNotification('textDocument/didOpen', {
    textDocument: { uri, languageId: 'typst', version: 1, text },
  });

  const coldStart = performance.now();
  server.onNotification('typst/compile', { uri });
  const coldMs = performance.now() - coldStart;
  server.drainEvents();

  // Type one character at the end, the way a person does.
  const samples: number[] = [];
  for (let step = 0; step < 20; step += 1) {
    text += 'x';
    server.onNotification('textDocument/didChange', {
      textDocument: { uri, version: 2 + step },
      contentChanges: [{ text }],
    });

    const started = performance.now();
    server.onNotification('typst/compile', { uri });
    samples.push(performance.now() - started);
    server.drainEvents();
  }

  const metrics = server.onRequest('typst/documentMetrics', { uri }) as {
    pageCount: number;
  };

  // Render every page once, to find the largest.
  const pages = Array.from({ length: metrics.pageCount }, (_, i) => i);
  const rendered = server.onRequest('typst/renderPages', {
    uri,
    pages,
    knownHashes: {},
  }) as { patches: { content?: string }[] };

  const largest = Math.max(
    0,
    ...rendered.patches.map((patch) => patch.content?.length ?? 0),
  );

  return {
    name,
    pages: metrics.pageCount,
    coldMs,
    keystrokeMs: median(samples.slice(10)),
    p95Ms: percentile(samples, 95),
    largestPageKb: largest / 1024,
    heapMb: wasm.TypstServer.heapBytes() / 1_048_576,
  };
}
