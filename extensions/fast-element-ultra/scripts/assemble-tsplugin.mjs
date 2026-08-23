// Assembles node_modules/wx-fast-element-tsplugin/ from the built bundle and
// the WASM artifact. tsserver resolves plugins only from a probe location's
// node_modules/<pluginName>, and VS Code probes the extension's install
// directory — so the plugin has to live there, both in the VSIX and in the
// working tree for F5 sessions (RFC 011, gate 1 / task P1-09).
//
// pnpm may prune this directory on install; every `pnpm run build` recreates
// it.

import { copyFileSync, mkdirSync, writeFileSync, existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const target = join(root, 'node_modules', 'wx-fast-element-tsplugin');
mkdirSync(target, { recursive: true });

const bundle = join(root, 'dist', 'tsplugin', 'index.js');
if (!existsSync(bundle)) {
  console.error('assemble-tsplugin: dist/tsplugin/index.js missing — run tsdown first');
  process.exit(1);
}
copyFileSync(bundle, join(target, 'index.js'));

for (const artifact of ['fast_analyzer_wasm.js', 'fast_analyzer_wasm_bg.wasm']) {
  const source = join(root, 'wasm', artifact);
  if (!existsSync(source)) {
    console.error(`assemble-tsplugin: wasm/${artifact} missing — run \`pnpm run build:wasm\` first`);
    process.exit(1);
  }
  copyFileSync(source, join(target, artifact));
}

// tsserver expects `require(plugin)` to *be* the factory function; the
// bundler emits it as the default export, so a two-line wrapper is the main.
writeFileSync(
  join(target, 'main.js'),
  `const factory = require('./index.js');\nmodule.exports = factory.default ?? factory;\n`,
);

writeFileSync(
  join(target, 'package.json'),
  `${JSON.stringify(
    {
      name: 'wx-fast-element-tsplugin',
      version: '0.1.0',
      description:
        'TypeScript server plugin for FAST Element templates — ships inside wx-vsce-fast-element-ultra.',
      main: 'main.js',
      private: true,
      license: 'MIT',
    },
    null,
    2,
  )}\n`,
);

console.log(`assembled ${target}`);
