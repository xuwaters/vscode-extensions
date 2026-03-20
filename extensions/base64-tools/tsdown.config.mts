import { defineConfig } from 'tsdown';

export default defineConfig({
  entry: ['src/extension.ts'],
  format: 'cjs',
  platform: 'node',
  outDir: 'dist',
  sourcemap: true,
  clean: true,
  external: ['vscode'],
});
