import * as fs from 'fs';
import * as path from 'path';
import { createRequire } from 'module';
import { defineConfig, type UserConfig } from 'tsdown';

const require = createRequire(import.meta.url);

/**
 * Loads .css files referenced via relative imports as default-export strings,
 * so the webview bundle inlines its own stylesheet instead of loading a second
 * file at runtime — while the CSS stays on disk as real CSS with real editor
 * support. Same pattern as typst-ultra and log-viewer.
 */
const rawAssetsPlugin = {
  name: 'raw-assets',
  async resolveId(source: string, importer: string | undefined) {
    if (!importer) return null;
    if (!/\.(html|css)$/.test(source)) return null;
    if (!source.startsWith('./') && !source.startsWith('../')) return null;
    return path.resolve(path.dirname(importer), source) + '?raw-asset';
  },
  load(id: string) {
    if (!id.endsWith('?raw-asset')) return null;
    const realPath = id.slice(0, -'?raw-asset'.length);
    return `export default ${JSON.stringify(fs.readFileSync(realPath, 'utf8'))};`;
  },
};

/**
 * pdf.js's out-of-bundle data, copied into `dist/pdfjs/`.
 *
 * These are read at runtime by URL rather than imported, so the bundler never
 * sees them, and a VSIX built without them renders a document with no embedded
 * fonts as blank boxes and a CJK document as tofu. What each directory buys:
 *
 * | Directory        | Needed for                                              |
 * | ---------------- | ------------------------------------------------------- |
 * | `cmaps`          | CID-keyed fonts — most CJK documents                      |
 * | `standard_fonts` | The 14 standard fonts, when a document does not embed them |
 * | `wasm`           | JBIG2 / JPEG 2000 image decoding, and ICC colour          |
 *
 * `quickjs-eval.*` is deliberately left behind: it is the engine for XFA forms
 * and document scripting, both of which this extension turns off. Shipping a
 * JavaScript interpreter that nothing can reach is 475 KB of attack surface for
 * no feature.
 */
const pdfjsAssetsPlugin = {
  name: 'pdfjs-assets',
  writeBundle() {
    const root = path.dirname(require.resolve('pdfjs-dist/package.json'));
    const out = path.resolve('dist', 'pdfjs');
    for (const dir of ['cmaps', 'standard_fonts', 'wasm']) {
      fs.cpSync(path.join(root, dir), path.join(out, dir), {
        recursive: true,
        filter: (src) => !path.basename(src).startsWith('quickjs-eval'),
      });
    }
    fs.copyFileSync(path.join(root, 'LICENSE'), path.join(out, 'LICENSE'));
  },
};

const config: UserConfig = [
  // Extension host bundle.
  {
    entry: ['src/extension.ts'],
    format: 'cjs',
    outExtensions: () => ({ js: '.js' }),
    platform: 'node',
    outDir: 'dist',
    sourcemap: true,
    clean: true,
    // `vsce package --no-dependencies` ships no node_modules, so everything the
    // host needs has to be inside the bundle. Only `vscode` is provided by the
    // host at runtime. pdf.js belongs to the webview and never loads here.
    deps: { neverBundle: ['vscode', 'pdfjs-dist'] },
    // Attached to the config that owns `clean` so the two are ordered within
    // one build. The asset tree is ~200 files; hung off a later config, its
    // copy could be swept away by a clean that had not run yet.
    plugins: [pdfjsAssetsPlugin],
  },
  // pdf.js's worker, as a classic (non-module) script.
  //
  // The webview cannot construct a worker straight from a `vscode-resource`
  // URL: worker scripts must be same-origin and that URL is not. So the page
  // fetches this bundle and constructs the worker from a blob of it, which is
  // same-origin by definition — and a blob worker has no base URL to resolve
  // imports against, hence `iife` rather than the ESM build's `import`s.
  {
    entry: { 'pdf.worker': 'webview/pdfWorker.ts' },
    format: 'iife',
    // Named outright rather than left to the format suffix: the host builds
    // this URL by hand, and `pdf.worker.iife.js` is not a name worth carrying
    // through the CSP comment and into the message protocol.
    outputOptions: { entryFileNames: '[name].js' },
    platform: 'browser',
    outDir: 'dist',
    sourcemap: false,
    deps: { alwaysBundle: [/.*/] },
    // pdf.js's JBIG2 and OpenJPEG modules carry an Emscripten preamble that
    // reads `import.meta.url` — into a `try`/`catch` whose result is discarded.
    // An IIFE has no `import.meta`, so it is substituted here rather than left
    // to rolldown's warning: the `new URL` throws into that catch and nothing
    // downstream notices.
    define: { 'import.meta': '{}' },
  },
  // Viewer webview bundle.
  {
    entry: { webview: 'webview/index.ts' },
    format: 'esm',
    outExtensions: () => ({ js: '.js' }),
    platform: 'browser',
    outDir: 'dist',
    sourcemap: true,
    deps: { alwaysBundle: [/.*/] },
    plugins: [rawAssetsPlugin],
  },
];

export default defineConfig(config);
