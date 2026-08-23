/**
 * The corpus gate (research/corpus.md §4): this repo's own FAST extensions
 * are the permanent regression corpus.
 *
 * 1. Discovery — every element found, with its @observable members, from
 *    `@customElement({ name: CONST })`: the case fast-analyzer misses.
 * 2. Zero false positives — the full rule set, strict on, over every
 *    template: silence is the expected result.
 * 3. Rename round-trip lives in features.test.ts.
 */

import * as path from 'node:path';

import * as ts from 'typescript';
import { describe, expect, it } from 'vitest';

import { createHarness, messages, repoRoot, type Harness, wasmBuilt } from './harness.js';

const CORPUS = [
  {
    extension: 'csv-ultra',
    tag: 'csv-grid',
    className: 'CsvGrid',
    lightDom: false,
    // A sample, not the full set — presence proves the member walk.
    expectObservables: ['hasHeader', 'wrap', 'readOnly', 'findOpen', 'findQuery', 'menu', 'sort'],
  },
  {
    extension: 'pdf-ultra',
    tag: 'pdf-viewer',
    className: 'PdfViewer',
    lightDom: false,
    expectObservables: [],
  },
  {
    extension: 'typst-ultra',
    tag: 'typst-preview',
    className: 'TypstPreview',
    lightDom: true,
    expectObservables: ['zoom', 'pageCount', 'inverted'],
  },
] as const;

function corpusFiles(extension: string): string[] {
  const viewer = path.join(repoRoot, 'extensions', extension, 'webview', 'viewer');
  return [path.join(viewer, 'element.ts'), path.join(viewer, 'template.ts')];
}

/** Compiler options matching the webview tsconfigs the corpus builds with. */
const CORPUS_OPTIONS: ts.CompilerOptions = {
  strict: true,
  target: ts.ScriptTarget.ES2022,
  module: ts.ModuleKind.ES2022,
  moduleResolution: ts.ModuleResolutionKind.Bundler,
  experimentalDecorators: true,
  useDefineForClassFields: false,
  lib: ['lib.es2022.d.ts', 'lib.dom.d.ts', 'lib.dom.iterable.d.ts'],
  resolveJsonModule: true,
  skipLibCheck: true,
  noEmit: true,
};

function corpusHarness(extension: string): Harness {
  return createHarness(
    {},
    {
      rootFiles: corpusFiles(extension),
      compilerOptions: CORPUS_OPTIONS,
      settings: { strict: true, logging: 'off' },
    },
  );
}

describe.skipIf(!wasmBuilt)('discovery over the real corpus', () => {
  for (const entry of CORPUS) {
    it(`finds <${entry.tag}> with its members (${entry.extension})`, () => {
      const harness = corpusHarness(entry.extension);
      harness.service.sync();
      const elementFile = corpusFiles(entry.extension)[0];
      const extraction = harness.service.fileExtraction(elementFile);
      expect(extraction, `no extraction for ${elementFile}`).toBeDefined();
      const component = extraction!.upsert.components.find((c) => c.tagName === entry.tag);
      expect(component, `<${entry.tag}> not discovered`).toBeDefined();
      expect(component!.className).toBe(entry.className);
      expect(component!.hasShadowRoot).toBe(!entry.lightDom);
      const propertyNames = component!.properties.map((p) => p.name);
      for (const expected of entry.expectObservables) {
        expect(propertyNames, `missing @observable ${expected}`).toContain(expected);
      }
      // The template in template.ts is linked to the component in element.ts.
      expect(component!.templateDocumentId).toBeTruthy();
      expect(component!.templateDocumentId).toContain('template.ts');
    });
  }
});

describe.skipIf(!wasmBuilt)('zero false positives over the corpus, strict mode on', () => {
  for (const entry of CORPUS) {
    for (const file of ['element.ts', 'template.ts'] as const) {
      it(`${entry.extension}/webview/viewer/${file} is silent`, () => {
        const harness = corpusHarness(entry.extension);
        const target = corpusFiles(entry.extension)[file === 'element.ts' ? 0 : 1];
        const diagnostics = harness.fastDiagnostics(target);
        expect(
          messages(diagnostics).map(
            (m, i) => `${diagnostics[i].code} @${diagnostics[i].start}: ${m}`,
          ),
        ).toEqual([]);
      });
    }
  }
});

describe.skipIf(!wasmBuilt)('corpus counts stay in step with research/corpus.md', () => {
  it('every corpus template is typed', () => {
    for (const entry of CORPUS) {
      const harness = corpusHarness(entry.extension);
      harness.service.sync();
      for (const file of corpusFiles(entry.extension)) {
        const extraction = harness.service.fileExtraction(file);
        if (!extraction) continue;
        for (const doc of extraction.upsert.documents) {
          if (doc.kind !== 'html') continue;
          expect(doc.sourceTypeName, `${doc.id} is untyped`).toBeTruthy();
        }
      }
    }
  });
});
