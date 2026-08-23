// Assembles node_modules/<PLUGIN_NAME>/ from the built bundle and the WASM
// artifact, so the plugin sits where tsserver's probe will find it — both in
// the VSIX and in the working tree for F5 sessions.
//
// pnpm may prune this directory on install; every `pnpm run build` recreates
// it.

import { copyFileSync, existsSync, mkdirSync, writeFileSync } from 'node:fs';
import { basename, join } from 'node:path';

import { reportMissing } from '../lib/checks.mjs';
import { PLUGIN_NAME, WASM_ARTIFACTS } from '../lib/layout.mjs';

/**
 * The manifest written into the assembled directory. Only `name` and `main`
 * are load-bearing — they are what tsserver's resolver reads; the rest keep
 * the directory a well-formed, unpublishable package.
 *
 * @typedef {object} PluginManifest
 * @property {string} name Must match `contributes.typescriptServerPlugins`.
 * @property {string} version
 * @property {string} description
 * @property {string} main Entry tsserver requires; the factory wrapper.
 * @property {true} private Guards against an accidental publish.
 * @property {string} license
 */

/**
 * tsserver expects `require(plugin)` to *be* the factory function; the bundler
 * emits it as the default export, so a two-line wrapper is the main.
 *
 * @type {string}
 */
const MAIN_WRAPPER = `const factory = require('./index.js');\nmodule.exports = factory.default ?? factory;\n`;

/** @type {import('../../../../scripts/lib/cli.mjs').Command<{ layout: import('../lib/layout.mjs').Layout }>} */
export const assembleTspluginCommand = {
  name: 'assemble-tsplugin',
  summary: `Build node_modules/${PLUGIN_NAME}/ from the bundle and the wasm artifacts`,

  details: [
    'Reads dist/tsplugin/index.js and wasm/, so `tsdown` and `pnpm run build:wasm`',
    'have to have run first. Part of `pnpm run build`.',
  ],

  run({ layout, write }) {
    /** @type {Array<{ from: string, to: string }>} */
    const copies = [
      { from: layout.bundle, to: join(layout.pluginDir, 'index.js') },
      ...WASM_ARTIFACTS.map((artifact) => ({
        from: layout.wasm(artifact),
        to: join(layout.pluginDir, artifact),
      })),
    ];

    const missing = copies.map((copy) => copy.from).filter((from) => !existsSync(from));
    const code = reportMissing(
      missing,
      'build inputs',
      missing.every((path) => basename(path).startsWith('fast_analyzer_wasm'))
        ? 'pnpm run build:wasm'
        : 'pnpm run build',
      write,
    );
    if (code !== 0) return code;

    mkdirSync(layout.pluginDir, { recursive: true });
    for (const { from, to } of copies) copyFileSync(from, to);

    writeFileSync(join(layout.pluginDir, 'main.js'), MAIN_WRAPPER);

    /** @type {PluginManifest} */
    const manifest = {
      name: PLUGIN_NAME,
      version: '0.1.0',
      description: `TypeScript server plugin for FAST Element templates — ships inside ${layout.manifest.name}.`,
      main: 'main.js',
      private: true,
      license: 'MIT',
    };
    writeFileSync(
      join(layout.pluginDir, 'package.json'),
      `${JSON.stringify(manifest, null, 2)}\n`,
    );

    write(`assembled ${layout.pluginDir}`);
    return 0;
  },
};
