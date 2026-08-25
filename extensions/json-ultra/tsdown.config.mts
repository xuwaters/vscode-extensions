import * as fs from 'fs';
import * as path from 'path';
import { defineConfig, type UserConfig } from 'tsdown';

/**
 * Loads .css files referenced via relative imports as default-export strings,
 * so the webview bundle inlines its own stylesheet instead of loading a second
 * file at runtime — same pattern as csv-ultra, pdf-ultra and log-viewer.
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
    // `vsce package --no-dependencies` ships no node_modules; only `vscode`
    // is provided by the host at runtime. The WASM bundle is reached via a
    // computed path, so it stays out of the bundle by construction.
    deps: { neverBundle: ['vscode'] },
  },
  // JSON Lines table webview bundle.
  {
    entry: { webview: 'webview/index.ts' },
    format: 'esm',
    outExtensions: () => ({ js: '.js' }),
    platform: 'browser',
    outDir: 'dist',
    sourcemap: true,
    deps: { alwaysBundle: [/.*/] },
    tsconfig: 'webview/tsconfig.json',
    plugins: [rawAssetsPlugin],
  },
];

export default defineConfig(config);
