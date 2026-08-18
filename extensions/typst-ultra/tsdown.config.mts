import * as fs from 'fs';
import * as path from 'path';
import { defineConfig, type UserConfig } from 'tsdown';

/**
 * Loads .css files referenced via relative imports as default-export strings,
 * so the webview bundle inlines its own stylesheet instead of loading a second
 * file at runtime — while the CSS stays on disk as real CSS with real editor
 * support. Same pattern as log-viewer.
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
    // `vsce package --no-dependencies` ships no node_modules, so the language
    // client has to be inside the bundle. Only `vscode` itself is provided by
    // the host at runtime.
    deps: { neverBundle: ['vscode'], alwaysBundle: [/^vscode-language/] },
  },
  // Language server bundle. Runs as a child process, so it is bundled
  // separately and started with `TransportKind.ipc` — the WASM heap and a
  // compiler panic both stay out of the extension host.
  {
    entry: { server: 'server/main.ts' },
    format: 'cjs',
    outExtensions: () => ({ js: '.js' }),
    platform: 'node',
    outDir: 'dist',
    sourcemap: true,
    // The WASM glue is loaded at runtime with a computed `require`, resolved
    // relative to the extension root — a 26 MB artifact has no business going
    // through the bundler. Everything else, including the LSP server library,
    // is bundled, because the VSIX carries no node_modules.
    deps: { neverBundle: ['vscode'], alwaysBundle: [/^vscode-language/] },
  },
  // Preview webview bundle.
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
    // decorators, so without `experimentalDecorators` the transform passes
    // `@customElement` through as a standard decorator: syntax no browser
    // engine parses, which fails the whole bundle and leaves an empty tab.
    // `useDefineForClassFields: false` matters for the same reason —
    // `@observable` installs accessors on the prototype, and a class field
    // with `[[Define]]` semantics would shadow them, leaving the chrome inert.
    tsconfig: 'webview/tsconfig.json',
    plugins: [rawAssetsPlugin],
  },
];

export default defineConfig(config);
