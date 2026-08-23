// Injects node_modules/wx-fast-element-tsplugin/ into the packaged VSIX.
//
// RFC 011, gate 1 (P1-09): tsserver only loads plugins from a probe
// location's node_modules/, and VS Code probes the extension's install
// directory — so the VSIX must contain the directory. The
// `.vscodeignore` negation (the gate's fallback 1) cannot work with current
// vsce: its file collection globs with `ignore: 'node_modules/**'`
// (@vscode/vsce out/package.js, collectAllFiles), so files under
// node_modules are never even offered to the ignore rules. This script is
// the gate's fallback 3 — rewrite the zip after packaging.
//
// A VSIX is a plain zip; entries live under `extension/`.

import { execFileSync } from 'node:child_process';
import { cpSync, mkdirSync, mkdtempSync, readdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const pluginDir = join(root, 'node_modules', 'wx-fast-element-tsplugin');

const vsix = readdirSync(root).find((f) => f.endsWith('.vsix'));
if (!vsix) {
  console.error('inject-tsplugin: no .vsix found — run vsce package first');
  process.exit(1);
}
const vsixPath = resolve(root, vsix);

const staging = mkdtempSync(join(tmpdir(), 'fast-element-vsix-'));
try {
  const target = join(staging, 'extension', 'node_modules', 'wx-fast-element-tsplugin');
  mkdirSync(target, { recursive: true });
  cpSync(pluginDir, target, { recursive: true });
  execFileSync('zip', ['-r', '-q', vsixPath, 'extension'], { cwd: staging });
} finally {
  rmSync(staging, { recursive: true, force: true });
}

const listing = execFileSync('unzip', ['-l', vsixPath], { encoding: 'utf8' });
const required = [
  'extension/node_modules/wx-fast-element-tsplugin/package.json',
  'extension/node_modules/wx-fast-element-tsplugin/main.js',
  'extension/node_modules/wx-fast-element-tsplugin/index.js',
  'extension/node_modules/wx-fast-element-tsplugin/fast_analyzer_wasm.js',
  'extension/node_modules/wx-fast-element-tsplugin/fast_analyzer_wasm_bg.wasm',
];
const missing = required.filter((entry) => !listing.includes(entry));
if (missing.length > 0) {
  console.error(`inject-tsplugin: VSIX is missing ${missing.join(', ')}`);
  process.exit(1);
}
console.log(`inject-tsplugin: ${vsix} now carries the tsserver plugin`);
