/**
 * P1-05, the parser gate: our tree against parse5's, over the corpus and an
 * adversarial set. Decision 0003 rests on this — a failure here means
 * switching to swc_html_parser and a superseding ADR.
 *
 * Deliberate divergences are enumerated as expectations of OUR behaviour, not
 * silently allowed:
 *  - unclosed tags stay unclosed (parse5 repairs the tree),
 *  - placeholders are first-class (parse5 sees underscore runs),
 *  - comments and doctypes are ignored in the comparison.
 */

import { createRequire } from 'node:module';
import * as path from 'node:path';

import * as parse5 from 'parse5';
import * as ts from 'typescript';
import { describe, expect, it } from 'vitest';

import type { PlaceholderFact } from '../tsplugin/protocol.js';
import { substitute } from '../tsplugin/extract.js';
import { createHarness, repoRoot, wasmBuilt, wasmGluePath } from './harness.js';

const require0 = createRequire(import.meta.url);
// eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
const wasm = wasmBuilt ? require0(wasmGluePath) : undefined;

interface OurNode {
  kind: string;
  name: string;
  attrs: string[];
  elementExpressions: number;
  closed: boolean;
  implied: boolean;
  selfClosing: boolean;
  children: OurNode[];
}

function ourTree(text: string, placeholders: PlaceholderFact[]): OurNode[] {
  const engine = new wasm.Engine();
  engine.setConfig('{}');
  const ok = engine.upsertFile(
    JSON.stringify({
      fileName: '/differential.ts',
      dependencies: [],
      components: [],
      documents: [
        {
          id: 'diff',
          fileName: '/differential.ts',
          templateStart: 0,
          kind: 'html',
          text,
          placeholders,
        },
      ],
    }),
  );
  expect(ok).toBe(true);
  const result = engine.query(JSON.stringify({ type: 'parseTree', documentId: 'diff' }));
  expect(result).toBeDefined();
  return JSON.parse(result!) as OurNode[];
}

/** parse5's fragment tree in the same comparable shape. */
function parse5Tree(html: string): OurNode[] {
  const fragment = parse5.parseFragment(html);
  interface P5Node {
    nodeName: string;
    tagName?: string;
    value?: string;
    attrs?: Array<{ name: string }>;
    childNodes?: P5Node[];
    content?: { childNodes: P5Node[] };
  }
  function convert(nodes: P5Node[]): OurNode[] {
    const out: OurNode[] = [];
    for (const node of nodes) {
      if (node.nodeName === '#text') {
        const content = (node.value ?? '').trim();
        if (content) {
          out.push({
            kind: 'text',
            name: content,
            attrs: [],
            elementExpressions: 0,
            closed: false,
            implied: false,
            selfClosing: false,
            children: [],
          });
        }
        continue;
      }
      if (node.nodeName === '#comment' || node.nodeName === '#documentType') continue;
      if (!node.tagName) continue;
      const children = node.content?.childNodes ?? node.childNodes ?? [];
      out.push({
        kind: 'element',
        name: node.tagName.toLowerCase(),
        attrs: (node.attrs ?? []).map((a) => a.name.toLowerCase()).sort(),
        elementExpressions: 0,
        closed: true,
        implied: false,
        selfClosing: false,
        children: convert(children),
      });
    }
    return out;
  }
  return convert((fragment as unknown as P5Node).childNodes ?? []);
}

/**
 * Structure only: names and nesting, in document order. Text is normalized —
 * whitespace collapsed, underscore runs canonicalized, adjacent runs merged —
 * because parse5 sees a placeholder as literal underscore text while our tree
 * keeps it as a node; the two are the same document.
 */
function shape(nodes: OurNode[]): unknown[] {
  const out: unknown[] = [];
  let pendingText: string[] = [];
  const flush = (): void => {
    const text = pendingText
      .join(' ')
      .replace(/_+[0-9a-z]*_+|_{2,}/g, '_')
      .replace(/\s+/g, ' ')
      .trim();
    pendingText = [];
    if (text) out.push({ text });
  };
  for (const n of nodes) {
    if (n.kind === 'element') {
      flush();
      out.push({ name: n.name, children: shape(n.children) });
    } else if (n.kind === 'text') {
      pendingText.push(n.name);
    } else {
      pendingText.push('_');
    }
  }
  flush();
  return out;
}

/** Shape plus attribute names, for inputs without element expressions. */
function shapeWithAttrs(nodes: OurNode[]): unknown[] {
  return nodes.map((n) =>
    n.kind === 'element'
      ? { name: n.name, attrs: n.attrs, children: shapeWithAttrs(n.children) }
      : { [n.kind]: n.kind === 'text' ? n.name.replace(/\s+/g, ' ') : true },
  );
}

describe.skipIf(!wasmBuilt)('well-formed inputs match parse5 exactly', () => {
  const CASES = [
    `<div class="a b" id="x"><span>text</span></div>`,
    `<ul><li>one</li><li>two</li></ul>`,
    `<svg viewBox="0 0 16 16"><path d="M2 3h12v3H2z" fill="currentColor"/><circle cx="7" cy="7" r="4"/></svg>`,
    `<table><thead><tr><th>h</th></tr></thead><tbody><tr><td>c</td></tr></tbody></table>`,
    `<style>.a > .b { color: red; }</style><div>after</div>`,
    `<textarea><div>not markup</div></textarea>`,
    `<input type="text" disabled><br><img src="x.png" alt="">`,
    `<p>one</p><p>two</p>`,
    `<div data-x="a &gt; b" title="q > r"><b>bold</b>text</div>`,
    `<button aria-pressed="false" tabindex="0">ok</button>`,
    `<select><option value="a">A</option><option value="b">B</option></select>`,
    `<svg><defs><linearGradient id="g"><stop offset="0"/></linearGradient></defs><rect width="4" height="4"/></svg>`,
    `<video controls muted playsinline></video>`,
    `<div><!-- a comment --><span>x</span></div>`,
  ];
  for (const input of CASES) {
    it(JSON.stringify(input.slice(0, 60)), () => {
      expect(shapeWithAttrs(ourTree(input, []))).toEqual(shapeWithAttrs(parse5Tree(input)));
    });
  }
});

describe.skipIf(!wasmBuilt)('implied end tags match parse5', () => {
  const CASES = [
    `<ul><li>a<li>b</ul>`,
    `<p>one<div>block</div>`,
    `<select><option>a<option>b</select>`,
    `<dl><dt>t<dd>d<dt>t2<dd>d2</dl>`,
  ];
  for (const input of CASES) {
    it(JSON.stringify(input), () => {
      expect(shape(ourTree(input, []))).toEqual(shape(parse5Tree(input)));
    });
  }
});

describe.skipIf(!wasmBuilt)('deliberate divergences, asserted as our behaviour', () => {
  it('no synthesized tbody: <tr> directly in <table> stays where it was written', () => {
    // The HTML5 tree builder wraps stray rows in an implied <tbody>; we keep
    // the written structure — a template's spans must index the source.
    const tree = ourTree('<table><tr><td>a<td>b<tr><td>c</table>', []);
    const table = tree[0];
    expect(table.name).toBe('table');
    expect(table.children.map((c) => c.name)).toEqual(['tr', 'tr']);
    expect(table.children[0].children.map((c) => c.name)).toEqual(['td', 'td']);
    for (const tr of table.children) {
      expect(tr.implied || tr.closed).toBe(true);
    }
  });

  it('unclosed tags stay unclosed instead of being repaired', () => {
    // parse5 turns <div><span></div> into div>span with both "closed"; we
    // keep the span open and record it, because no-unclosed-tag needs to see
    // what was written.
    const tree = ourTree('<div><span></div>', []);
    expect(tree).toHaveLength(1);
    const span = tree[0].children[0];
    expect(span.name).toBe('span');
    expect(span.closed).toBe(false);
    expect(span.implied).toBe(false);
  });

  it('self-closed non-void HTML elements parse as closed but are flagged', () => {
    // parse5 ignores the slash and leaves <div/> OPEN (swallowing following
    // content); treating it as closed is the deliberate divergence, because
    // the rule reports the self-close itself.
    const tree = ourTree('<div/><span>x</span>', []);
    expect(tree.map((n) => n.name)).toEqual(['div', 'span']);
    expect(tree[0].selfClosing).toBe(true);
  });

  it('placeholders parse in attribute-name position', () => {
    const source = `<div class="table" \${ref('tableEl')} tabindex="0">x</div>`;
    const placeholders = placeholdersOf(source);
    const substituted = substitute(source, placeholders);
    const tree = ourTree(substituted, placeholders);
    expect(tree[0].elementExpressions).toBe(1);
    expect(tree[0].attrs).toEqual(['class', 'tabindex']);
    // parse5, by contrast, sees the underscore run as an attribute — which
    // is exactly why the substitution is a legal attribute name.
    const p5 = parse5Tree(substituted);
    expect(p5[0].attrs.length).toBe(3);
  });
});

describe.skipIf(!wasmBuilt)('the corpus, template by template', () => {
  const extensions = ['csv-ultra', 'pdf-ultra', 'typst-ultra'];
  for (const extension of extensions) {
    it(`${extension}: every template parses to the same shape as parse5`, () => {
      const viewer = path.join(repoRoot, 'extensions', extension, 'webview', 'viewer');
      const harness = createHarness(
        {},
        {
          rootFiles: [path.join(viewer, 'element.ts'), path.join(viewer, 'template.ts')],
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
      harness.service.sync();
      let compared = 0;
      for (const file of [path.join(viewer, 'element.ts'), path.join(viewer, 'template.ts')]) {
        const extraction = harness.service.fileExtraction(file);
        if (!extraction) continue;
        for (const doc of extraction.upsert.documents) {
          if (doc.kind !== 'html') continue;
          const ours = ourTree(doc.text, doc.placeholders);
          const theirs = parse5Tree(doc.text);
          expect(shape(ours), doc.id).toEqual(shape(theirs));
          compared += 1;
        }
      }
      expect(compared).toBeGreaterThan(0);
    });
  }
});

/** Extract `${…}` regions the way the test fixtures write them. */
function placeholdersOf(source: string): PlaceholderFact[] {
  const out: PlaceholderFact[] = [];
  let index = 0;
  for (let i = 0; i < source.length; i++) {
    if (source[i] === '$' && source[i + 1] === '{') {
      let depth = 0;
      let j = i;
      for (; j < source.length; j++) {
        if (source[j] === '{') depth += 1;
        if (source[j] === '}') {
          depth -= 1;
          if (depth === 0) {
            j += 1;
            break;
          }
        }
      }
      out.push({ index: index++, start: i, end: j });
      i = j - 1;
    }
  }
  return out;
}
