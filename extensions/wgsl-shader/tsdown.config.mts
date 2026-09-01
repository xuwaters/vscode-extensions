import { defineConfig, type UserConfig } from 'tsdown';

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
  // Language server bundle. Runs as a child process, so it is built separately
  // and started with `TransportKind.ipc` — the WASM heap and a naga panic both
  // stay out of the extension host.
  {
    entry: { server: 'server/main.ts' },
    format: 'cjs',
    outExtensions: () => ({ js: '.js' }),
    platform: 'node',
    outDir: 'dist',
    sourcemap: true,
    // The WASM glue is loaded at runtime with a computed `require`, resolved
    // relative to the extension root; the bundler leaves it alone. Everything
    // else, the LSP server library included, is bundled, because the VSIX
    // carries no node_modules.
    deps: { neverBundle: ['vscode'], alwaysBundle: [/^vscode-language/] },
  },
];

export default defineConfig(config);
