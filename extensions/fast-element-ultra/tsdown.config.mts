import { defineConfig, type UserConfig } from 'tsdown';

const config: UserConfig = [
  // Extension host bundle. `vsce package --no-dependencies` ships no
  // node_modules, so everything the host needs is inside the bundle; only
  // `vscode` is provided at runtime.
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
  // The TypeScript server plugin. Runs inside tsserver, which passes its own
  // `typescript` module to the factory — the bundle must never carry one.
  // vscode-css-languageservice and the textdocument shim are bundled; the
  // WASM glue is loaded at runtime relative to the assembled plugin
  // directory (scripts/assemble-tsplugin.mjs).
  {
    entry: { index: 'tsplugin/index.ts' },
    format: 'cjs',
    outExtensions: () => ({ js: '.js' }),
    platform: 'node',
    outDir: 'dist/tsplugin',
    sourcemap: true,
    deps: { neverBundle: ['typescript'], alwaysBundle: [/^vscode-/] },
  },
];

export default defineConfig(config);
