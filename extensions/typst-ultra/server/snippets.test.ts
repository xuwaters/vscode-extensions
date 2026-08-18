import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import { pathToFileURL } from 'url';
import { describe, expect, it } from 'vitest';
import { FontIndex } from './fonts.js';
import { BUNDLED_FONTS } from './testFonts.js';
import { listDir, readFile, type Roots } from './vfs.js';

/**
 * The bundled snippets, compiled by the real typst.
 *
 * A snippet that does not compile is worse than no snippet: the beginner it
 * was written for cannot tell their mistake from ours. So every body here is
 * expanded to its default text and run through the same engine the extension
 * ships, and any *error* diagnostic fails the suite.
 *
 * This lives beside the other engine tests rather than in `src/` because the
 * WASM harness does — the file under test is `snippets/typst.json` at the
 * extension root.
 */

const ROOT = path.join(__dirname, '..');
const WASM = path.join(ROOT, 'wasm', 'typst_lsp_wasm.js');
const BUILT = fs.existsSync(WASM);

interface Snippet {
  prefix: string | string[];
  body: string[];
  description: string;
}

const SNIPPETS = JSON.parse(
  fs.readFileSync(path.join(ROOT, 'snippets', 'typst.json'), 'utf8'),
) as Record<string, Snippet>;

/**
 * How a snippet has to be framed before it is a document.
 *
 * Most are documents already. Math fragments are meant to be typed between
 * dollars; a few name a value or a label the user is expected to supply, so
 * the test supplies one; two import from the registry, which the test host
 * deliberately cannot reach.
 */
const FRAMING: Record<
  string,
  { math?: true; skip?: string; before?: string; after?: string }
> = {
  Fraction: { math: true },
  'Square root': { math: true },
  Sum: { math: true },
  Product: { math: true },
  Integral: { math: true },
  Limit: { math: true },
  Matrix: { math: true },
  Vector: { math: true },
  'Cases (piecewise)': { math: true },
  'Upright text inside math': { math: true },
  'Cross-reference': { after: '\n\n#figure([x], caption: [c]) <fig:name>' },
  Label: { after: '\n\n= Section' },
  Citation: { after: '\n\n#bibliography("refs.bib", style: "ieee")' },
  'Variable binding': { before: '#let value = 1\n' },
  Conditional: { before: '#let condition = true\n' },
  'While loop': { before: '#let condition = false\n' },
  'Import a Universe package': { skip: 'downloads from the package registry' },
  'Slides scaffold (touying)': { skip: 'downloads from the package registry' },
};

/** Snippets that reference a path, and the fixtures those paths need. */
const FIXTURES: Record<string, string | Uint8Array> = {
  'refs.bib': '@book{key, title = {A Book}, author = {An Author}, year = {2026}}\n',
  'utils.typ': '#let greet(name) = [Hello, #name!]\n',
  'chapters/01-intro.typ': '= Introduction\n',
  'chapters/02-method.typ': '= Method\n',
  'src/main.rs': 'fn main() {}\n',
  // The smallest PNG that decodes: 1×1, 8-bit RGB.
  'images/plot.png': Buffer.from(
    'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=',
    'base64',
  ),
};

/**
 * Expand a snippet body to what the editor would leave behind if the user
 * pressed Escape at the first tab stop: every placeholder replaced by its
 * default, every escape unescaped.
 */
function materialize(body: string[]): string {
  const text = body.join('\n');
  let out = '';

  for (let i = 0; i < text.length; i++) {
    const char = text[i];

    if (char === '\\') {
      const next = text[i + 1];
      if (next === '$' || next === '\\' || next === '}') {
        out += next;
        i += 1;
      } else {
        out += char;
      }
      continue;
    }

    if (char === '$') {
      const rest = text.slice(i);
      const placeholder = /^\$\{\d+:([^}]*)\}/.exec(rest);
      if (placeholder) {
        out += placeholder[1];
        i += placeholder[0].length - 1;
        continue;
      }
      const bare = /^(\$\{\d+\}|\$\d+)/.exec(rest);
      if (bare) {
        i += bare[0].length - 1;
        continue;
      }
    }

    out += char;
  }

  return out;
}

describe('the bundled snippets, as data', () => {
  it('is what package.json points at', () => {
    const manifest = JSON.parse(
      fs.readFileSync(path.join(ROOT, 'package.json'), 'utf8'),
    ) as { contributes: { snippets: { language: string; path: string }[] } };

    const contributed = manifest.contributes.snippets;
    expect(contributed).toContainEqual({
      language: 'typst',
      path: './snippets/typst.json',
    });
    for (const entry of contributed) {
      expect(fs.existsSync(path.join(ROOT, entry.path))).toBe(true);
    }
  });

  it('describes every snippet, so the completion list explains itself', () => {
    for (const [name, snippet] of Object.entries(SNIPPETS)) {
      expect(snippet.description, name).toBeTruthy();
      expect(snippet.body.length, name).toBeGreaterThan(0);
      expect(snippet.prefix, name).toBeTruthy();
    }
  });

  it('claims each prefix once', () => {
    const owner = new Map<string, string>();
    for (const [name, snippet] of Object.entries(SNIPPETS)) {
      for (const prefix of [snippet.prefix].flat()) {
        expect(owner.get(prefix) ?? name, `prefix "${prefix}"`).toBe(name);
        owner.set(prefix, name);
      }
    }
  });

  it('uses prefixes typst’s word pattern can actually match', () => {
    // The language configuration's `wordPattern` stops at `-`, `#`, `.` and
    // friends, so a prefix containing one would never be typed to completion.
    for (const [name, snippet] of Object.entries(SNIPPETS)) {
      for (const prefix of [snippet.prefix].flat()) {
        expect(prefix, name).toMatch(/^[A-Za-z][A-Za-z0-9]*$/);
      }
    }
  });

  it('escapes every literal dollar, so math snippets are not tab stops', () => {
    for (const [name, snippet] of Object.entries(SNIPPETS)) {
      const text = snippet.body.join('\n');
      for (let i = 0; i < text.length; i++) {
        if (text[i] === '\\') {
          i += 1;
          continue;
        }
        if (text[i] !== '$') continue;
        expect(text.slice(i, i + 12), `${name}: unescaped $`).toMatch(
          /^\$(\{\d+[:}]|\d+)/,
        );
      }
    }
  });
});

describe.skipIf(!BUILT)('the bundled snippets, through typst', () => {
  it('compiles every one of them without an error', () => {
    const workspace = fs.mkdtempSync(path.join(os.tmpdir(), 'typst-snippets-'));
    try {
      for (const [name, content] of Object.entries(FIXTURES)) {
        const target = path.join(workspace, name);
        fs.mkdirSync(path.dirname(target), { recursive: true });
        fs.writeFileSync(target, content);
      }

      // eslint-disable-next-line @typescript-eslint/no-require-imports
      const wasm = require(WASM);
      const roots: Roots = { project: workspace, packageCache: '' };
      const fonts = new FontIndex('', (data: Uint8Array) =>
        wasm.TypstServer.indexFont(data),
      );
      fonts.addDirectories([BUNDLED_FONTS]);

      const host = {
        readFile: (root: string, vpath: string) => readFile(roots, root, vpath),
        listDir: (root: string, vpath: string) => listDir(roots, root, vpath),
        fontData: (face: number) => fonts.data(face),
        resolvePackage: () => 'failed:packages are disabled in this test',
        now: () => Date.UTC(2026, 7, 17, 12, 0, 0),
        timezoneOffsetMinutes: () => 0,
      };

      const rootUri = pathToFileURL(workspace).toString();
      const mainUri = `${rootUri}/main.typ`;
      fs.writeFileSync(path.join(workspace, 'main.typ'), '');

      const server = new wasm.TypstServer(host, {
        rootUri,
        mainPath: 'main.typ',
        settings: {},
        fontFaces: fonts.descriptors,
        packages: [],
      });

      server.onNotification('textDocument/didOpen', {
        textDocument: { uri: mainUri, languageId: 'typst', version: 1, text: '' },
      });

      const failures: string[] = [];
      let version = 1;
      let compiled = 0;

      for (const [name, snippet] of Object.entries(SNIPPETS)) {
        const framing = FRAMING[name] ?? {};
        if (framing.skip) continue;

        const expanded = materialize(snippet.body);
        const source =
          (framing.before ?? '') +
          (framing.math ? `$ ${expanded} $` : expanded) +
          (framing.after ?? '') +
          '\n';

        version += 1;
        server.onNotification('textDocument/didChange', {
          textDocument: { uri: mainUri, version },
          contentChanges: [{ text: source }],
        });
        server.onNotification('typst/compile', { uri: mainUri });

        const errors = server
          .drainEvents()
          .filter(
            (event: { method: string }) =>
              event.method === 'textDocument/publishDiagnostics',
          )
          .flatMap(
            (event: { params: { diagnostics: { severity: number; message: string }[] } }) =>
              event.params.diagnostics,
          )
          .filter((diagnostic: { severity: number }) => diagnostic.severity === 1);

        compiled += 1;
        if (errors.length > 0) {
          failures.push(
            `${name}: ${errors.map((e: { message: string }) => e.message).join('; ')}`,
          );
        }
      }

      expect(failures).toEqual([]);
      expect(compiled).toBeGreaterThan(50);
    } finally {
      fs.rmSync(workspace, { recursive: true, force: true });
    }
  });
});
