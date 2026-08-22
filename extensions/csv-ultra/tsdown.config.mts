import * as fs from 'fs';
import * as path from 'path';
import { defineConfig, type UserConfig } from 'tsdown';

/**
 * Loads .css files referenced via relative imports as default-export strings,
 * so the webview bundle inlines its own stylesheet instead of loading a second
 * file at runtime — while the CSS stays on disk as real CSS with real editor
 * support. Same pattern as pdf-ultra, typst-ultra and log-viewer.
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
    // host at runtime.
    deps: { neverBundle: ['vscode'] },
  },
  // Grid webview bundle.
  {
    entry: { webview: 'webview/index.ts' },
    format: 'esm',
    outExtensions: () => ({ js: '.js' }),
    platform: 'browser',
    outDir: 'dist',
    sourcemap: true,
    deps: { alwaysBundle: [/.*/] },
    // The webview's own tsconfig, not the package root's — and it is
    // load-bearing, not tidiness. `@microsoft/fast-element` ships *legacy*
    // decorators, and the root tsconfig leaves `experimentalDecorators` off, so
    // the transform would pass `@customElement` through as a standard
    // decorator: syntax no browser engine parses. The bundle then fails to load
    // as a whole, the bootstrap never runs, and the tab is an empty `<body>`
    // with no `<csv-grid>` in it. `bundle.test.ts` is the guard.
    tsconfig: 'webview/tsconfig.json',
    plugins: [rawAssetsPlugin],
  },
];

export default defineConfig(config);
