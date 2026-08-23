// Where the packaged pieces live and what the TypeScript server plugin is
// called. Every command reads its paths from a `Layout`, so the plugin's name,
// its file list, and the VSIX filename are each stated once — and so tests can
// point the same commands at a temporary directory.
//
// tsserver resolves plugins only from a probe location's node_modules/<name>,
// and VS Code probes the extension's install directory (RFC 011, gate 1 /
// task P1-09). That constraint is why `pluginDir` sits under node_modules/
// rather than dist/, and why `vsixEntry` exists at all.

import { createRequire } from 'node:module';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);

/** Extension root inferred from this file: scripts/lib/layout.mjs -> ../.. */
export const EXTENSION_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');

/**
 * The name tsserver probes for. Must match the name in the manifest's
 * `contributes.typescriptServerPlugins`.
 *
 * @type {string}
 */
export const PLUGIN_NAME = 'wx-fast-element-tsplugin';

/** wasm-pack outputs the engine loads at runtime, copied beside the bundle. */
export const WASM_ARTIFACTS = /** @type {readonly string[]} */ ([
  'fast_analyzer_wasm.js',
  'fast_analyzer_wasm_bg.wasm',
]);

/**
 * Every file the assembled plugin directory must contain. `assemble` writes
 * this set, `inject` checks the VSIX for it, and `verify` loads through it.
 *
 * @type {readonly string[]}
 */
export const PLUGIN_FILES = ['package.json', 'main.js', 'index.js', ...WASM_ARTIFACTS];

/**
 * The two manifest fields vsce composes the archive filename from.
 *
 * @typedef {object} ExtensionManifest
 * @property {string} name
 * @property {string} version
 */

/**
 * @typedef {object} Layout
 * @property {string} root Extension root, the directory holding package.json.
 * @property {ExtensionManifest} manifest
 * @property {string} pluginDir Assembled plugin: <root>/node_modules/<PLUGIN_NAME>.
 * @property {string} bundle Plugin bundle tsdown emits: <root>/dist/tsplugin/index.js.
 * @property {string} vsixName Archive filename, composed exactly as vsce composes it.
 * @property {string} vsixPath Absolute path to that archive.
 * @property {(...parts: string[]) => string} path Path inside the extension.
 * @property {(artifact: string) => string} wasm Path to a wasm-pack output in <root>/wasm.
 * @property {(file: string) => string} vsixEntry Zip entry path for a plugin file.
 */

/**
 * @param {string} [root] Extension root; defaults to this checkout.
 * @returns {Layout}
 */
export function createLayout(root = EXTENSION_ROOT) {
  /** @type {ExtensionManifest} */
  const manifest = require(join(root, 'package.json'));

  // Name the VSIX from the manifest rather than scanning for `*.vsix`: a stale
  // build of an older version would otherwise be picked up in its place.
  const vsixName = `${manifest.name}-${manifest.version}.vsix`;

  return {
    root,
    manifest,
    vsixName,
    vsixPath: join(root, vsixName),
    pluginDir: join(root, 'node_modules', PLUGIN_NAME),
    bundle: join(root, 'dist', 'tsplugin', 'index.js'),
    path: (...parts) => join(root, ...parts),
    wasm: (artifact) => join(root, 'wasm', artifact),
    // A VSIX is a plain zip whose entries all live under `extension/`.
    vsixEntry: (file) => `extension/node_modules/${PLUGIN_NAME}/${file}`,
  };
}
