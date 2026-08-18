import * as child_process from 'child_process';
import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import { pathToFileURL } from 'url';
import { describe, expect, it } from 'vitest';
import { FontIndex } from './fonts.js';
import { BUNDLED_FONTS } from './testFonts.js';
import { readFile, listDir, type Roots } from './vfs.js';

/**
 * P3-05: measure the transport half of the preview latency budget.
 *
 * The engine half was measured by the feasibility spike and is comfortable. The
 * transport half — JSON-RPC serialization, Node IPC, `postMessage`, and DOM
 * parsing — was *estimated* at ~50 ms of a ~65 ms budget, and the RFC named it
 * the biggest unknown in the design.
 *
 * The answer is that the wire is nearly free: **~4 ms** for a 394 KB page,
 * against ~30 ms estimated. Details and the full record are in
 * `docs/rfc/010-typst-ultra/research/transport.md`.
 *
 * Two things this deliberately does not measure, and does not pretend to:
 *
 * * **Paint.** What a compositor does with 3,000 `<use>` elements needs a real
 *   browser; `happy-dom` does no layout at all.
 * * **VSCode's webview channel.** `postMessage` crosses a boundary VSCode owns.
 *   `structuredClone` of the same payload is the closest honest proxy, and is
 *   labelled as one.
 */

const WASM = path.join(__dirname, '..', 'wasm', 'typst_lsp_wasm.js');
const BUILT = fs.existsSync(WASM);

/**
 * The budget preview.md §8 allocated to serialization plus the two hops.
 *
 * The original estimate was ~15 ms for JSON-RPC and Node IPC plus ~15 ms for
 * `postMessage`. This is that estimate, kept as the ceiling the regression test
 * enforces.
 */
const TRANSPORT_BUDGET_MS = 30;

interface Measurement {
  name: string;
  ms: number;
}

function median(values: number[]): number {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.floor(sorted.length / 2)];
}

function time(runs: number, action: () => void): number {
  // One warm-up so JIT compilation is not billed to the first measurement.
  action();
  const samples: number[] = [];
  for (let i = 0; i < runs; i += 1) {
    const started = performance.now();
    action();
    samples.push(performance.now() - started);
  }
  return median(samples);
}

describe.skipIf(!BUILT)('preview transport budget', () => {
  it('stays well inside the budget the design allocates it', async () => {
    const workspace = fs.mkdtempSync(path.join(os.tmpdir(), 'typst-transport-'));
    try {
      const rendered = renderRealPage(workspace);

      // The spike measured 386 KB for its densest A4 page. Measure at that size
      // too, so the result is directly comparable to the estimate it replaces
      // rather than to a smaller page that would flatter it.
      const svg = padTo(rendered, 386 * 1024);
      const bytes = Buffer.byteLength(svg, 'utf8');
      expect(bytes).toBeGreaterThan(380_000);

      const response = { patches: [{ op: 'replace', index: 0, hash: 'a'.repeat(16), svg }] };
      const wire = JSON.stringify(response);

      const measurements: Measurement[] = [
        {
          name: 'JSON-RPC serialize (server side)',
          ms: time(20, () => void JSON.stringify(response)),
        },
        {
          name: 'JSON-RPC parse (host side)',
          ms: time(20, () => void JSON.parse(wire)),
        },
        {
          name: 'Node IPC round trip (send + echo)',
          ms: await measureIpc(wire),
        },
        {
          name: 'structuredClone (postMessage proxy)',
          ms: time(20, () => void structuredClone(response)),
        },
      ];

      const wireTotal = measurements.reduce((sum, m) => sum + m.ms, 0);
      const domMs = await measureDomParse(svg);

      console.log(
        `\nrendered page: ${(Buffer.byteLength(rendered, 'utf8') / 1024).toFixed(0)} KB` +
          `, measured at ${(bytes / 1024).toFixed(0)} KB`,
      );
      for (const { name, ms } of measurements) {
        console.log(`  ${name.padEnd(46)} ${ms.toFixed(1)} ms`);
      }
      console.log(`  ${'wire total'.padEnd(46)} ${wireTotal.toFixed(1)} ms`);
      console.log(`  ${'DOMParser + adopt (happy-dom, indicative)'.padEnd(46)} ${domMs.toFixed(1)} ms\n`);

      // Only the wire half is asserted. It is what this process can measure
      // meaningfully, and it is where the design's risk was said to be: the
      // estimate was ~30 ms for serialization plus two hops.
      //
      // The DOM number is reported but not asserted, because `happy-dom` is a
      // pure-JS parser and is not comparable to Chromium's in either direction —
      // it is probably slower at parsing, and it does no paint at all. Closing
      // that half properly needs a browser, and is what P4-08 should carry.
      expect(wireTotal).toBeLessThan(TRANSPORT_BUDGET_MS);
    } finally {
      fs.rmSync(workspace, { recursive: true, force: true });
    }
  }, 60_000);
});

/** Compile a lorem-heavy page with the real engine and render it to SVG. */
function renderRealPage(workspace: string): string {
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  const wasm = require(WASM);

  const text = '= Section\n\n#lorem(160)\n\n$ sum_(i=1)^n i = (n(n+1))/2 $\n\n#lorem(120)\n';
  fs.writeFileSync(path.join(workspace, 'main.typ'), text);

  const roots: Roots = { project: workspace, packageCache: '' };
  const fonts = new FontIndex('', (data: Uint8Array) => wasm.TypstServer.indexFont(data));
  fonts.addDirectories([BUNDLED_FONTS]);

  const server = new wasm.TypstServer(
    {
      readFile: (root: string, vpath: string) => readFile(roots, root, vpath),
      listDir: (root: string, vpath: string) => listDir(roots, root, vpath),
      fontData: (face: number) => fonts.data(face),
      resolvePackage: () => 'failed:not needed',
      now: () => Date.UTC(2026, 7, 17),
      timezoneOffsetMinutes: () => 0,
    },
    {
      rootUri: pathToFileURL(workspace).toString(),
      mainPath: 'main.typ',
      settings: {},
      fontFaces: fonts.descriptors,
      packages: [],
    },
  );

  const uri = `${pathToFileURL(workspace).toString()}/main.typ`;
  server.onNotification('textDocument/didOpen', {
    textDocument: { uri, languageId: 'typst', version: 1, text },
  });
  server.onNotification('typst/compile', { uri });
  server.drainEvents();

  const result = server.onRequest('typst/renderPages', {
    uri,
    pages: [0],
    knownHashes: {},
  }) as { patches: { content?: string }[] };

  const svg = result.patches[0]?.content;
  if (!svg) throw new Error('the fixture did not render a page');
  return svg;
}

/**
 * Grow a page's body to a target size by repeating its `<use>` elements.
 *
 * `<use>` positioning is 88% of a real page's bytes, so repeating it produces a
 * payload with the same shape as a denser page rather than an artificial one.
 */
function padTo(svg: string, targetBytes: number): string {
  const body = svg.match(/<use\b[^>]*\/>/g)?.join('') ?? '';
  if (!body) return svg;

  let out = svg;
  while (Buffer.byteLength(out, 'utf8') < targetBytes) {
    out = out.replace(/<\/svg>\s*$/, `${body}</svg>`);
    if (body.length === 0) break;
  }
  return out;
}

/**
 * Parse the page SVG and run the adopt-time stripping over it.
 *
 * Indicative only. `happy-dom` is a pure-JS DOM and is not comparable to a
 * browser engine in either direction: its parser is almost certainly slower
 * than Chromium's, and it does no paint at all — which is the part that would
 * actually matter for 3,000 `<use>` elements.
 */
async function measureDomParse(svg: string): Promise<number> {
  const { Window } = await import('happy-dom');
  const window = new Window();
  const parser = new window.DOMParser();

  return time(5, () => {
    const parsed = parser.parseFromString(svg, 'image/svg+xml');
    const root = parsed.documentElement ?? parsed.firstElementChild;
    root?.querySelectorAll('script, foreignObject').forEach((node) => node.remove());
  });
}

/**
 * A real Node IPC round trip, which is the transport `TransportKind.ipc` uses.
 *
 * Forking is billed once and excluded; what is timed is the message crossing.
 */
async function measureIpc(payload: string): Promise<number> {
  const child = child_process.spawn(
    process.execPath,
    ['-e', 'process.on("message", (m) => process.send(m));'],
    { stdio: ['ignore', 'ignore', 'ignore', 'ipc'] },
  );

  try {
    const once = () =>
      new Promise<number>((resolve, reject) => {
        const started = performance.now();
        const onMessage = () => {
          child.off('message', onMessage);
          resolve(performance.now() - started);
        };
        child.on('message', onMessage);
        child.send(payload, (error) => {
          if (error) reject(error);
        });
      });

    await once(); // warm-up
    const samples: number[] = [];
    for (let i = 0; i < 10; i += 1) samples.push(await once());
    return median(samples);
  } finally {
    child.kill();
  }
}
