/**
 * P5-04: the measurements. Opt-in — `FAST_MEASURE=1 pnpm vitest run
 * test/measure.test.ts` — because numbers belong in research/, produced on
 * purpose, not silently in every CI run. Results print as a markdown table.
 */

import { createRequire } from 'node:module';
import * as path from 'node:path';

import * as ts from 'typescript';
import { describe, expect, it } from 'vitest';

import { createHarness, extensionRoot, repoRoot, wasmBuilt } from './harness.js';

const require0 = createRequire(import.meta.url);
const enabled = process.env.FAST_MEASURE === '1' && wasmBuilt;

describe.runIf(enabled)('measurements (P5-04)', () => {
  it('produces the numbers for research/measurements.md', () => {
    const lines: string[] = [];
    const wasmPath = path.join(extensionRoot, 'wasm', 'fast_analyzer_wasm.js');

    // -- artifact ---------------------------------------------------------
    const fs = require0('node:fs') as typeof import('node:fs');
    const zlib = require0('node:zlib') as typeof import('node:zlib');
    const wasmBytes = fs.readFileSync(
      path.join(extensionRoot, 'wasm', 'fast_analyzer_wasm_bg.wasm'),
    );
    lines.push(`artifact size: ${wasmBytes.length} bytes raw, ${zlib.gzipSync(wasmBytes).length} gzipped`);

    // -- module load + instantiation --------------------------------------
    const t0 = process.hrtime.bigint();
    // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
    const wasm = require0(wasmPath);
    const t1 = process.hrtime.bigint();
    const engines = [];
    for (let i = 0; i < 10; i++) engines.push(new wasm.Engine());
    const t2 = process.hrtime.bigint();
    lines.push(`module require+compile: ${ms(t0, t1)} ms`);
    lines.push(`Engine construction: ${(Number(t2 - t1) / 1e6 / 10).toFixed(3)} ms each`);

    // -- engine-only analyze over the biggest corpus template -------------
    const viewer = path.join(repoRoot, 'extensions', 'pdf-ultra', 'webview', 'viewer');
    const harness = createHarness(
      {},
      {
        rootFiles: [path.join(viewer, 'element.ts'), path.join(viewer, 'template.ts')],
        compilerOptions: corpusOptions(),
        settings: { strict: true },
      },
    );
    const templateFile = path.join(viewer, 'template.ts');

    const cold0 = process.hrtime.bigint();
    const coldDiags = harness.decorated.getSemanticDiagnostics(templateFile);
    const cold1 = process.hrtime.bigint();
    lines.push(
      `cold getSemanticDiagnostics (pdf-ultra template.ts, program+extraction+engine+oracle): ${ms(cold0, cold1)} ms, ${coldDiags.length} diagnostics`,
    );

    const warm0 = process.hrtime.bigint();
    const WARM_RUNS = 20;
    for (let i = 0; i < WARM_RUNS; i++) {
      harness.decorated.getSemanticDiagnostics(templateFile);
    }
    const warm1 = process.hrtime.bigint();
    lines.push(
      `warm getSemanticDiagnostics: ${(Number(warm1 - warm0) / 1e6 / WARM_RUNS).toFixed(2)} ms each over ${WARM_RUNS} runs`,
    );

    // -- binding-fact batch sizes (budget 3) ------------------------------
    harness.service.sync();
    for (const ext of ['csv-ultra', 'pdf-ultra']) {
      const v = path.join(repoRoot, 'extensions', ext, 'webview', 'viewer');
      const h = createHarness(
        {},
        {
          rootFiles: [path.join(v, 'element.ts'), path.join(v, 'template.ts')],
          compilerOptions: corpusOptions(),
          settings: { strict: true },
        },
      );
      h.service.sync();
      const extraction = h.service.fileExtraction(path.join(v, 'template.ts'));
      let facts = 0;
      let docs = 0;
      for (const doc of extraction?.upsert.documents ?? []) {
        if (doc.kind !== 'html') continue;
        docs += 1;
        facts += h.engine.analyze(doc.id)?.facts.length ?? 0;
      }
      lines.push(`binding-fact batch, ${ext} template.ts: ${facts} facts across ${docs} documents (one crossing per document each way)`);
    }

    // -- growth over 1,000 edits ------------------------------------------
    const engine = new wasm.Engine();
    engine.setConfig('{"strict":true}');
    const template = '<div>' + '<button class="b" aria-label="x">ok</button>'.repeat(60) + '</div>';
    const before = process.memoryUsage().rss;
    const editStart = process.hrtime.bigint();
    for (let i = 0; i < 1000; i++) {
      engine.upsertFile(
        JSON.stringify({
          fileName: '/edit.ts',
          dependencies: [],
          components: [],
          documents: [
            {
              id: 'e',
              fileName: '/edit.ts',
              templateStart: 0,
              kind: 'html',
              text: template + ' '.repeat(i % 7),
              placeholders: [],
            },
          ],
        }),
      );
      engine.analyze('e');
    }
    const editEnd = process.hrtime.bigint();
    const after = process.memoryUsage().rss;
    lines.push(
      `1,000 upsert+analyze cycles over a ${template.length}-byte template: ${(Number(editEnd - editStart) / 1e6 / 1000).toFixed(3)} ms per cycle, rss ${(before / 1e6).toFixed(0)} → ${(after / 1e6).toFixed(0)} MB`,
    );

    console.log(`\n${lines.map((l) => `- ${l}`).join('\n')}\n`);
    fs.writeFileSync('/tmp/claude/fast-measure.txt', lines.join('\n'));
    expect(lines.length).toBeGreaterThan(5);
  });
});

it.runIf(!enabled)('measurements are opt-in (FAST_MEASURE=1)', () => {
  expect(true).toBe(true);
});

function corpusOptions(): ts.CompilerOptions {
  return {
    strict: true,
    target: ts.ScriptTarget.ES2022,
    module: ts.ModuleKind.ES2022,
    moduleResolution: ts.ModuleResolutionKind.Bundler,
    experimentalDecorators: true,
    useDefineForClassFields: false,
    lib: ['lib.es2022.d.ts', 'lib.dom.d.ts', 'lib.dom.iterable.d.ts'],
    skipLibCheck: true,
    noEmit: true,
  };
}

function ms(a: bigint, b: bigint): string {
  return (Number(b - a) / 1e6).toFixed(2);
}
