/**
 * Phase 4 exit: the position features, driven through the decorated language
 * service against fixtures and the real corpus. The two features that justify
 * the project — `ref('…')` completion and member rename reaching template
 * strings — are tested on csv-ultra's real files.
 */

import * as path from 'node:path';

import * as ts from 'typescript';
import { describe, expect, it } from 'vitest';

import { createHarness, fixture, repoRoot, type Harness, wasmBuilt } from './harness.js';

const FILE = fixture('feature.ts');

function harnessWith(body: string): Harness {
  return createHarness({ [FILE]: body }, { settings: { strict: true } });
}

function offsetOf(harness: Harness, file: string, needle: string, shift = 0): number {
  const text = harness.ls.getProgram()!.getSourceFile(file)!.text;
  const index = text.indexOf(needle);
  expect(index, `needle not found: ${needle}`).toBeGreaterThanOrEqual(0);
  return index + shift;
}

const PRELUDE = `
  import { FASTElement, customElement, observable, attr, html, ref } from '@microsoft/fast-element';
  export const GRID_TAG = 'my-grid';
  const template = html<MyGrid>\`<div></div>\`;
  /** The grid. */
  @customElement({ name: GRID_TAG, template })
  export class MyGrid extends FASTElement {
    /** Whether the first row is a header. */
    @observable hasHeader = false;
    @attr({ mode: 'boolean' }) locked = false;
    findInput!: HTMLInputElement;
  }
`;

describe.skipIf(!wasmBuilt)('completion', () => {
  it('offers component tags, attributes and prefixed forms inside a template', () => {
    const harness = harnessWith(
      `${PRELUDE}
      export const use = html<MyGrid>\`<my-grid ></my-grid>\`;`,
    );
    const position = offsetOf(harness, FILE, '<my-grid ></my-grid>', '<my-grid '.length);
    const completions = harness.decorated.getCompletionsAtPosition(FILE, position, undefined);
    const names = completions!.entries.map((e) => e.name);
    expect(names).toContain('locked');
    expect(names).toContain(':hasHeader');
    expect(names).toContain('?locked');
    expect(names).toContain('@click');
    expect(names).toContain('class');
  });

  it('completes inside ref(…) with the source type’s members', () => {
    const harness = harnessWith(
      `${PRELUDE}
      export const use = html<MyGrid>\`<input \${ref('findInput')} />\`;`,
    );
    const position = offsetOf(harness, FILE, `ref('findInput')`, `ref('fi`.length);
    const completions = harness.decorated.getCompletionsAtPosition(FILE, position, undefined);
    expect(completions).toBeDefined();
    const names = completions!.entries.map((e) => e.name);
    expect(names).toContain('findInput');
    expect(names).toContain('hasHeader');
    // The replacement span covers exactly the string contents.
    const entry = completions!.entries.find((e) => e.name === 'findInput')!;
    const text = harness.ls.getProgram()!.getSourceFile(FILE)!.text;
    const span = entry.replacementSpan!;
    expect(text.slice(span.start, span.start + span.length)).toBe('findInput');
  });

  it('inside an ordinary placeholder, TypeScript still answers', () => {
    const harness = harnessWith(
      `${PRELUDE}
      export const use = html<MyGrid>\`<span>\${(x) => x.hasHeader}</span>\`;`,
    );
    const position = offsetOf(harness, FILE, 'x.hasHeader}</span>', 'x.'.length + 1);
    const completions = harness.decorated.getCompletionsAtPosition(FILE, position, undefined);
    // TypeScript's member completion for MyGrid, not tag names.
    expect(completions?.entries.some((e) => e.name === 'hasHeader')).toBe(true);
    expect(completions?.entries.some((e) => e.name === '$emit')).toBe(true);
  });
});

describe.skipIf(!wasmBuilt)('hover, definition, references', () => {
  it('hovers a component tag with its documentation', () => {
    const harness = harnessWith(
      `${PRELUDE}
      export const use = html<MyGrid>\`<my-grid></my-grid>\`;`,
    );
    const position = offsetOf(harness, FILE, '<my-grid></my-grid>', 3);
    const info = harness.decorated.getQuickInfoAtPosition(FILE, position);
    expect(info).toBeDefined();
    const docs = (info!.documentation ?? []).map((d) => d.text).join('');
    expect(docs).toContain('MyGrid');
    expect(docs).toContain('The grid.');
  });

  it('definition on an attribute lands on the member declaration', () => {
    const harness = harnessWith(
      `${PRELUDE}
      export const use = html<MyGrid>\`<my-grid locked></my-grid>\`;`,
    );
    const position = offsetOf(harness, FILE, 'locked></my-grid>', 2);
    const result = harness.decorated.getDefinitionAndBoundSpan(FILE, position);
    expect(result?.definitions).toHaveLength(1);
    const target = result!.definitions![0];
    const text = harness.ls.getProgram()!.getSourceFile(FILE)!.text;
    expect(text.slice(target.textSpan.start, target.textSpan.start + target.textSpan.length)).toBe(
      'locked',
    );
    // And it is the declaration, not the binding.
    expect(target.textSpan.start).toBeLessThan(position);
  });

  it('an event binding hovers its detail type and goes to the $events entry', () => {
    const harness = harnessWith(`
      import { FASTElement, customElement, html } from '@microsoft/fast-element';
      const bar = html<TabBar>\`<div></div>\`;
      @customElement({ name: 'tab-bar', template: bar })
      export class TabBar extends FASTElement {
        declare $events: {
          /** A tab was chosen. */
          'tab-select': { id: number };
        };
      }
      @customElement({ name: 'tab-host', template: null as never })
      export class TabHost extends FASTElement {}
      export const use = html<TabHost>\`<tab-bar @tab-select="\${(x, c) => c.event}"></tab-bar>\`;
    `);
    const position = offsetOf(harness, FILE, '@tab-select="', 3);
    const info = harness.decorated.getQuickInfoAtPosition(FILE, position);
    const docs = (info?.documentation ?? []).map((d) => d.text).join('');
    expect(docs).toContain('CustomEvent<{ id: number; }>');
    expect(docs).toContain('A tab was chosen.');

    const result = harness.decorated.getDefinitionAndBoundSpan(FILE, position);
    const target = result!.definitions![0];
    const text = harness.ls.getProgram()!.getSourceFile(FILE)!.text;
    expect(text.slice(target.textSpan.start, target.textSpan.start + target.textSpan.length)).toBe(
      'tab-select',
    );
    // The declaration in the map, not the binding being hovered.
    expect(target.textSpan.start).toBeLessThan(position);
  });

  it('find-all-references from a tag finds every template use', () => {
    const harness = harnessWith(
      `${PRELUDE}
      export const a = html<MyGrid>\`<my-grid></my-grid>\`;
      export const b = html<MyGrid>\`<div><my-grid locked></my-grid></div>\`;`,
    );
    const position = offsetOf(harness, FILE, '<my-grid></my-grid>', 3);
    const groups = harness.decorated.findReferences(FILE, position);
    const ours = groups?.flatMap((g) => g.references) ?? [];
    // 2 open + 2 close tags.
    expect(ours.length).toBeGreaterThanOrEqual(4);
  });
});

describe.skipIf(!wasmBuilt)('rename', () => {
  it('renaming a member from a template binding renames the declaration too', () => {
    const harness = harnessWith(
      `${PRELUDE}
      export const use = html<MyGrid>\`<my-grid :hasHeader="\${(x) => x.hasHeader}"></my-grid>\`;`,
    );
    const position = offsetOf(harness, FILE, ':hasHeader=', 3);
    const info = harness.decorated.getRenameInfo(FILE, position, {});
    expect(info.canRename).toBe(true);
    const locations = harness.decorated.findRenameLocations(FILE, position, false, false, {});
    expect(locations).toBeDefined();
    const text = harness.ls.getProgram()!.getSourceFile(FILE)!.text;
    const renamed = locations!.map((l) =>
      text.slice(l.textSpan.start, l.textSpan.start + l.textSpan.length),
    );
    // The binding, the declaration, and the arrow-body use.
    expect(renamed.every((r) => r === 'hasHeader')).toBe(true);
    expect(locations!.length).toBeGreaterThanOrEqual(3);
    const declPosition = offsetOf(harness, FILE, '@observable hasHeader', '@observable '.length);
    expect(locations!.some((l) => l.textSpan.start === declPosition)).toBe(true);
  });

  it('renaming a tag reaches every template and the registration string', () => {
    const harness = harnessWith(
      `${PRELUDE}
      export const use = html<MyGrid>\`<my-grid></my-grid>\`;`,
    );
    const position = offsetOf(harness, FILE, `'my-grid'`, 1);
    const info = harness.decorated.getRenameInfo(FILE, position, {});
    expect(info.canRename).toBe(true);
    const locations = harness.decorated.findRenameLocations(FILE, position, false, false, {});
    // Open tag, close tag, registration string.
    expect(locations!.length).toBe(3);
  });
});

describe.skipIf(!wasmBuilt)('closing tags and folding', () => {
  it('completes the closing tag after >', () => {
    const harness = harnessWith(
      `${PRELUDE}
      export const use = html<MyGrid>\`<my-grid>\`;`,
    );
    const position = offsetOf(harness, FILE, '<my-grid>`;', '<my-grid>'.length);
    const result = harness.decorated.getJsxClosingTagAtPosition(FILE, position);
    expect(result?.newText).toBe('</my-grid>');
  });

  it('folds multiline elements and the template itself', () => {
    const harness = harnessWith(
      `${PRELUDE}
      export const use = html<MyGrid>\`
        <div>
          <span>x</span>
        </div>
      \`;`,
    );
    const spans = harness.decorated.getOutliningSpans(FILE);
    const ours = spans.filter((s) => s.bannerText === '…');
    // The template plus the div.
    expect(ours.length).toBeGreaterThanOrEqual(2);
  });
});

describe.skipIf(!wasmBuilt)('the real corpus (csv-ultra)', () => {
  const viewer = path.join(repoRoot, 'extensions', 'csv-ultra', 'webview', 'viewer');
  const elementFile = path.join(viewer, 'element.ts');
  const templateFile = path.join(viewer, 'template.ts');

  function corpusHarness(): Harness {
    return createHarness(
      {},
      {
        rootFiles: [elementFile, templateFile],
        compilerOptions: {
          strict: true,
          target: ts.ScriptTarget.ES2022,
          module: ts.ModuleKind.ES2022,
          moduleResolution: ts.ModuleResolutionKind.Bundler,
          experimentalDecorators: true,
          useDefineForClassFields: false,
          lib: ['lib.es2022.d.ts', 'lib.dom.d.ts', 'lib.dom.iterable.d.ts'],
          skipLibCheck: true,
          noEmit: true,
        },
        settings: { strict: true },
      },
    );
  }

  it("completion inside ref('findInput') offers CsvGrid's members", () => {
    const harness = corpusHarness();
    const position = offsetOf(harness, templateFile, `ref('findInput')`, `ref('fi`.length);
    const completions = harness.decorated.getCompletionsAtPosition(
      templateFile,
      position,
      undefined,
    );
    expect(completions).toBeDefined();
    const names = completions!.entries.map((e) => e.name);
    expect(names).toContain('findInput');
    expect(names).toContain('tableEl');
  });

  it("definition on ref('findInput') lands on the member in element.ts", () => {
    const harness = corpusHarness();
    const position = offsetOf(harness, templateFile, `ref('findInput')`, `ref('fi`.length);
    const result = harness.decorated.getDefinitionAndBoundSpan(templateFile, position);
    expect(result?.definitions?.[0]?.fileName).toBe(elementFile);
  });

  it("renaming findInput from the ref('…') string edits both files", () => {
    const harness = corpusHarness();
    const position = offsetOf(harness, templateFile, `ref('findInput')`, `ref('fi`.length);
    const locations = harness.decorated.findRenameLocations(
      templateFile,
      position,
      false,
      false,
      {},
    );
    expect(locations).toBeDefined();
    const files = new Set(locations!.map((l) => l.fileName));
    expect(files.has(templateFile)).toBe(true);
    expect(files.has(elementFile)).toBe(true);
    // Every renamed span is exactly the member's name.
    for (const location of locations!) {
      const text = harness.ls.getProgram()!.getSourceFile(location.fileName)!.text;
      expect(
        text.slice(location.textSpan.start, location.textSpan.start + location.textSpan.length),
      ).toBe('findInput');
    }
  });

  it('renaming findInput from its declaration reaches the template string', () => {
    const harness = corpusHarness();
    const position = offsetOf(harness, elementFile, 'findInput!: HTMLInputElement', 2);
    const locations = harness.decorated.findRenameLocations(elementFile, position, false, false, {});
    expect(locations).toBeDefined();
    const files = new Set(locations!.map((l) => l.fileName));
    expect(files.has(templateFile)).toBe(true);
  });

  it('hover on a DOM event binding explains the event', () => {
    const harness = corpusHarness();
    const position = offsetOf(harness, templateFile, '@click=', 2);
    const info = harness.decorated.getQuickInfoAtPosition(templateFile, position);
    expect(info).toBeDefined();
    const docs = (info!.documentation ?? []).map((d) => d.text).join('');
    expect(docs.toLowerCase()).toContain('pressed');
  });
});
