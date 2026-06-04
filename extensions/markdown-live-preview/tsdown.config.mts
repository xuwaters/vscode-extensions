import * as fs from 'fs';
import * as path from 'path';
import { createRequire } from 'module';
import { fileURLToPath } from 'url';
import { defineConfig } from 'tsdown';

const require = createRequire(import.meta.url);
const here = path.dirname(fileURLToPath(import.meta.url));

/**
 * `import 'katex/dist/katex.min.css'` inlines KaTeX's CSS into the emitted
 * style.css, but its `url(fonts/KaTeX_*.woff2)` references stay relative.
 * Copy the font files alongside style.css (dist/webview/fonts) so the
 * webview — which only loads from this folder — can resolve them.
 */
const copyKatexFonts = {
  name: 'copy-katex-fonts',
  async writeBundle() {
    const fontsDir = path.join(
      path.dirname(require.resolve('katex/package.json')),
      'dist',
      'fonts',
    );
    const outDir = path.join(here, 'dist', 'webview', 'fonts');
    await fs.promises.mkdir(outDir, { recursive: true });
    for (const file of await fs.promises.readdir(fontsDir)) {
      await fs.promises.copyFile(
        path.join(fontsDir, file),
        path.join(outDir, file),
      );
    }
  },
};

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
    outExtensions: () => ({ js: '.js' }),
    platform: 'browser',
    outDir: 'dist/webview',
    sourcemap: true,
    // Bundle all dependencies into the webview
    deps: { alwaysBundle: [/.*/] },
    plugins: [copyKatexFonts],
  },
]);
