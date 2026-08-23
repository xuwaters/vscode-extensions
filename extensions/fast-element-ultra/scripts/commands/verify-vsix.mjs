// Verifies the packaged VSIX the way tsserver would use it: extract to a clean
// directory (the installed-extension layout), resolve the plugin from
// <extension>/node_modules/<PLUGIN_NAME> exactly as tsserver's probe does, call
// the factory with the real TypeScript, and run the engine.
//
// This is the headless half of P1-09/P5-06. The other half — install into a
// real VS Code and read "Enabling plugin wx-fast-element-tsplugin" in the TS
// Server log — needs a desktop and stays a manual step.

import { existsSync, realpathSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';

import { createChecks, reportMissing } from '../lib/checks.mjs';
import { PLUGIN_NAME } from '../lib/layout.mjs';
import { extractZip, withTempDir } from '../lib/zip.mjs';

const require = createRequire(import.meta.url);

/**
 * The part of the extension manifest this command inspects.
 *
 * @typedef {object} PackagedManifest
 * @property {{ typescriptServerPlugins?: Array<{ name: string }> }} [contributes]
 */

/**
 * What tsserver calls once, to get the object it then decorates with.
 *
 * @typedef {(mod: { typescript: unknown }) => { create?: unknown }} PluginFactory
 */

/**
 * The analyzer's WASM surface, as much of it as this smoke test drives.
 *
 * @typedef {object} AnalyzerEngine
 * @property {(json: string) => boolean} setConfig
 * @property {(json: string) => boolean} upsertFile Adds or replaces one file's documents.
 * @property {(documentId: string) => string} analyze Returns `Analysis` as JSON.
 */

/**
 * @typedef {object} Analysis
 * @property {Array<{ ruleId: string }>} diagnostics
 */

/**
 * One unclosed tag, which `no-unclosed-tag` must report. Kept minimal: this
 * checks that the packaged engine runs at all, not what it finds.
 *
 * @type {string}
 */
const SAMPLE_FILE = JSON.stringify({
  fileName: '/v.ts',
  dependencies: [],
  components: [],
  documents: [
    {
      id: 'v',
      fileName: '/v.ts',
      templateStart: 0,
      kind: 'html',
      text: '<div><butto>x</div>',
      placeholders: [],
    },
  ],
});

/** @type {import('../../../../scripts/lib/cli.mjs').Command<{ layout: import('../lib/layout.mjs').Layout }>} */
export const verifyVsixCommand = {
  name: 'verify-vsix',
  summary: 'Load the packaged VSIX the way tsserver would, and run the engine from it',

  details: [
    'Extracts the archive named by the manifest, resolves the plugin through the',
    'probe path, and analyzes one document. Reports every check, and exits non-zero',
    'if any of them did not hold.',
  ],

  run({ layout, write }) {
    const inputs = reportMissing(
      existsSync(layout.vsixPath) ? [] : [layout.vsixName],
      'the packaged archive',
      'pnpm run package',
      write,
    );
    if (inputs !== 0) return inputs;

    const { check, exitCode } = createChecks(write);

    return withTempDir('fast-element-verify-', (extracted) => {
      extractZip(layout.vsixPath, extracted);
      const extensionDir = join(extracted, 'extension');

      // 1. The manifest declares the plugin tsserver should probe for.
      /** @type {PackagedManifest} */
      const manifest = require(join(extensionDir, 'package.json'));
      const declared = manifest.contributes?.typescriptServerPlugins?.[0]?.name;
      check(declared === PLUGIN_NAME, `manifest declares plugin: ${declared}`);

      // 2. tsserver's probe: resolve <probeLocation>/node_modules/<name>.
      const pluginMain = require.resolve(PLUGIN_NAME, { paths: [extensionDir] });
      check(
        realpathSync(pluginMain).startsWith(realpathSync(extensionDir)),
        `plugin resolves inside the extension: ${pluginMain}`,
      );

      // 3. The factory loads and decorates against the real TypeScript.
      /** @type {PluginFactory} */
      const factory = require(pluginMain);
      if (check(typeof factory === 'function', 'require(plugin) is the factory function')) {
        const pluginModule = factory({ typescript: require('typescript') });
        check(typeof pluginModule.create === 'function', 'factory returns { create }');
      }

      // 4. The engine next to it instantiates and analyzes.
      const { Engine } = /** @type {{ Engine: new () => AnalyzerEngine }} */ (
        require(join(dirname(pluginMain), 'fast_analyzer_wasm.js'))
      );
      const engine = new Engine();
      check(engine.setConfig('{"strict":true}'), 'engine accepts config');
      check(engine.upsertFile(SAMPLE_FILE), 'engine accepts a document');

      /** @type {Analysis} */
      const analysis = JSON.parse(engine.analyze('v'));
      check(
        analysis.diagnostics.some((d) => d.ruleId === 'no-unclosed-tag'),
        'engine analyzes from the packaged artifact',
      );

      if (exitCode() === 0) {
        write(`${layout.vsixName} passes — plugin resolvable, factory loads, engine analyzes`);
      }
      return exitCode();
    });
  },
};
