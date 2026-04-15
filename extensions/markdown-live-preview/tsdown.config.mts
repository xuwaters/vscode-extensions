import { defineConfig } from 'tsdown';

export default defineConfig([
  // Extension host bundle
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
  // Webview bundle
  {
    entry: ['webview/index.ts'],
    format: 'esm',
    platform: 'browser',
    outDir: 'dist/webview',
    sourcemap: true,
    // Bundle all dependencies into the webview
    deps: { alwaysBundle: [/.*/] },
  },
]);
