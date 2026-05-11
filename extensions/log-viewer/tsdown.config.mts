import * as fs from 'fs';
import * as path from 'path';
import { defineConfig, type UserConfig } from 'tsdown';

/**
 * Loads .html and .css files referenced via relative imports as default-export
 * strings. Lets the webview bundle inline its own HTML body and CSS without
 * loading separate files at runtime — and without giving up real editor
 * support (each asset stays on disk as its own file).
 */
const rawAssetsPlugin = {
  name: 'raw-assets',
  async resolveId(source: string, importer: string | undefined) {
    if (!importer) return null;
    if (!/\.(html|css)$/.test(source)) return null;
    if (!source.startsWith('./') && !source.startsWith('../')) return null;
    const resolved = path.resolve(path.dirname(importer), source);
    return resolved + '?raw-asset';
  },
  load(id: string) {
    if (!id.endsWith('?raw-asset')) return null;
    const realPath = id.slice(0, -'?raw-asset'.length);
    const src = fs.readFileSync(realPath, 'utf8');
    return `export default ${JSON.stringify(src)};`;
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
    deps: { neverBundle: ['vscode'] },
  },
  // Indexer worker bundle (Node worker_threads). Bundled separately so the
  // extension host can spawn it with `new Worker(<bundle>)`.
  {
    entry: { indexerWorker: 'src/indexer/worker.ts' },
    format: 'cjs',
    outExtensions: () => ({ js: '.js' }),
    platform: 'node',
    outDir: 'dist',
    sourcemap: true,
    deps: { neverBundle: ['vscode'] },
  },
  // Webview bundle: pulls in styles.css and body.html as inline strings.
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
