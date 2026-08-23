// Injects node_modules/<PLUGIN_NAME>/ into the packaged VSIX.
//
// RFC 011, gate 1 (P1-09): tsserver only loads plugins from a probe location's
// node_modules/, and VS Code probes the extension's install directory — so the
// VSIX must contain the directory. The `.vscodeignore` negation (the gate's
// fallback 1) cannot work with current vsce: its file collection globs with
// `ignore: 'node_modules/**'` (@vscode/vsce out/package.js, collectAllFiles),
// so files under node_modules are never even offered to the ignore rules. This
// command is the gate's fallback 3 — rewrite the zip after packaging.

import { cpSync, existsSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';

import { reportMissing } from '../lib/checks.mjs';
import { PLUGIN_FILES, PLUGIN_NAME } from '../lib/layout.mjs';
import { addToZip, missingFromZip, withTempDir } from '../lib/zip.mjs';

/** @type {import('../../../../scripts/lib/cli.mjs').Command<{ layout: import('../lib/layout.mjs').Layout }>} */
export const injectTspluginCommand = {
  name: 'inject-tsplugin',
  summary: `Add node_modules/${PLUGIN_NAME}/ to the packaged VSIX`,

  details: [
    'Runs after `vsce package`, on the archive named by the manifest. Rewrites the',
    'VSIX in place, then re-reads it to confirm every plugin file landed.',
  ],

  run({ layout, write }) {
    const inputs = reportMissing(
      [layout.vsixPath, layout.pluginDir].filter((path) => !existsSync(path)),
      'packaging inputs',
      'pnpm run package',
      write,
    );
    if (inputs !== 0) return inputs;

    withTempDir('fast-element-vsix-', (staging) => {
      const target = join(staging, 'extension', 'node_modules', PLUGIN_NAME);
      mkdirSync(target, { recursive: true });
      cpSync(layout.pluginDir, target, { recursive: true });
      addToZip(layout.vsixPath, staging, 'extension');
    });

    const missing = missingFromZip(layout.vsixPath, PLUGIN_FILES.map(layout.vsixEntry));
    if (missing.length > 0) {
      write(`${layout.vsixName} is missing ${missing.join(', ')}`);
      return 1;
    }

    write(`${layout.vsixName} now carries the tsserver plugin`);
    return 0;
  },
};
