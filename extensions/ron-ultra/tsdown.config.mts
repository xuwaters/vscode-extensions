import { defineConfig, type UserConfig } from 'tsdown';

const config: UserConfig = [
  // Extension host bundle. `vsce package --no-dependencies` ships no
  // node_modules, so the language client goes inside the bundle; only
  // `vscode` itself is provided at runtime.
  {
    entry: ['src/extension.ts'],
    format: 'cjs',
    outExtensions: () => ({ js: '.js' }),
    platform: 'node',
    outDir: 'dist',
    sourcemap: true,
    clean: true,
    deps: { neverBundle: ['vscode'], alwaysBundle: [/^vscode-language/] },
  },
  // Language server bundle, started as a child process over IPC. The WASM
  // glue is loaded at runtime with a computed `require` and left on disk.
  {
    entry: { server: 'server/main.ts' },
    format: 'cjs',
    outExtensions: () => ({ js: '.js' }),
    platform: 'node',
    outDir: 'dist',
    sourcemap: true,
    deps: { neverBundle: ['vscode'], alwaysBundle: [/^vscode-language/] },
  },
];

export default defineConfig(config);
