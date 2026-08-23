// Verifies the packaged VSIX the way tsserver would use it: extract to a
// clean directory (the installed-extension layout), resolve the plugin from
// <extension>/node_modules/<pluginName> exactly as tsserver's probe does,
// call the factory with the real TypeScript, and run the engine.
//
// This is the headless half of P1-09/P5-06. The other half — install into a
// real VS Code and read "Enabling plugin wx-fast-element-tsplugin" in the TS
// Server log — needs a desktop and stays a manual step.

import { execFileSync } from 'node:child_process';
import { mkdtempSync, readdirSync, realpathSync, rmSync } from 'node:fs';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const root = join(dirname(fileURLToPath(import.meta.url)), '..');

const vsix = readdirSync(root).find((f) => f.endsWith('.vsix'));
if (!vsix) {
  console.error('verify-vsix: no .vsix found — run `pnpm run package` first');
  process.exit(1);
}

const extracted = mkdtempSync(join(tmpdir(), 'fast-element-verify-'));
let failed = false;
try {
  execFileSync('unzip', ['-q', join(root, vsix), '-d', extracted]);
  const extensionDir = join(extracted, 'extension');

  // 1. The manifest declares the plugin tsserver should probe for.
  const manifest = require(join(extensionDir, 'package.json'));
  const declared = manifest.contributes?.typescriptServerPlugins?.[0]?.name;
  assert(declared === 'wx-fast-element-tsplugin', `manifest declares plugin: ${declared}`);

  // 2. tsserver's probe: resolve <probeLocation>/node_modules/<name>.
  const pluginMain = require.resolve('wx-fast-element-tsplugin', {
    paths: [extensionDir],
  });
  assert(
    realpathSync(pluginMain).startsWith(realpathSync(extensionDir)),
    `plugin resolves inside the extension: ${pluginMain}`,
  );

  // 3. The factory loads and decorates against the real TypeScript.
  const factory = require(pluginMain);
  assert(typeof factory === 'function', 'require(plugin) is the factory function');
  const ts = require('typescript');
  const pluginModule = factory({ typescript: ts });
  assert(typeof pluginModule.create === 'function', 'factory returns { create }');

  // 4. The engine next to it instantiates and analyzes.
  const wasm = require(join(dirname(pluginMain), 'fast_analyzer_wasm.js'));
  const engine = new wasm.Engine();
  assert(engine.setConfig('{"strict":true}'), 'engine accepts config');
  assert(
    engine.upsertFile(
      JSON.stringify({
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
      }),
    ),
    'engine accepts a document',
  );
  const analysis = JSON.parse(engine.analyze('v'));
  assert(
    analysis.diagnostics.some((d) => d.ruleId === 'no-unclosed-tag'),
    'engine analyzes from the packaged artifact',
  );

  console.log(`verify-vsix: ${vsix} passes — plugin resolvable, factory loads, engine analyzes`);
} finally {
  rmSync(extracted, { recursive: true, force: true });
  if (failed) process.exit(1);
}

function assert(condition, what) {
  if (condition) {
    console.log(`  ok: ${what}`);
  } else {
    console.error(`  FAILED: ${what}`);
    failed = true;
  }
}
