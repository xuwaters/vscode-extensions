import { defineConfig } from 'tsdown';

export default defineConfig({
  entry: ['src/extension.ts'],
  format: 'cjs',
  outExtensions: () => ({ js: '.js' }),
  platform: 'node',
  outDir: 'dist',
  sourcemap: true,
  clean: true,
  deps: { neverBundle: ['vscode'] },
});
