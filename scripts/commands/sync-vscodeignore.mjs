// Every extension packages the same way, so every extension wants the same
// .vscodeignore. This copies one template over all of them.
//
// An extension with packaging needs of its own — fast-element-ultra must ship
// its TypeScript server plugin inside node_modules/, which the template's
// defaults would drop — declares them in a `.vscodeignore-extra` file next to
// its package.json. The sync appends that file to the template, so the
// exception survives every future sync instead of being erased by it.

import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { toNameList } from '../lib/repo.mjs';
import { report } from '../lib/report.mjs';

/** scripts/commands/sync-vscodeignore.mjs -> scripts/templates/vscodeignore */
export const TEMPLATE_PATH = join(
  dirname(fileURLToPath(import.meta.url)),
  '..',
  'templates',
  '.vscodeignore',
);

/** @type {import('../lib/cli.mjs').Command} */
export const syncVscodeignoreCommand = {
  name: 'sync-vscodeignore',
  summary: 'Copy the shared .vscodeignore template into every extension',

  usage: [
    'repo sync-vscodeignore [--filter <name,...>] [--dry-run]',
    'repo sync-vscodeignore --check',
  ],

  options: {
    check: {
      type: 'boolean',
      describe: 'Exit non-zero if any copy differs from the template. Writes nothing.',
    },
    'dry-run': {
      type: 'boolean',
      short: 'n',
      describe: 'Print what would change without writing.',
    },
    filter: {
      type: 'string',
      short: 'f',
      multiple: true,
      placeholder: '<name,...>',
      describe: 'Extension directory names to sync. Repeatable. Default: all.',
    },
  },

  details: [
    'The template lives in scripts/templates/.vscodeignore. Edit it there — a copy',
    'edited in place is overwritten the next time this runs.',
    '',
    'A per-extension .vscodeignore-extra file, when present, is appended to the',
    'template for that extension — the mechanism for packaging exceptions.',
  ],

  run({ values, repo, write }) {
    const check = values.check === true;
    const dryRun = check || values['dry-run'] === true;
    const template = readFileSync(TEMPLATE_PATH, 'utf8');
    const targets = repo.resolveTargets(toNameList(values.filter));

    /** @type {import('../lib/report.mjs').ReportRow[]} */
    const rows = targets.map((name) => {
      const path = repo.path(name, '.vscodeignore');
      const current = readFileIfExists(path);
      const extra = readFileIfExists(repo.path(name, '.vscodeignore-extra'));
      const expected =
        extra === undefined
          ? template
          : `${template}\n# --- from .vscodeignore-extra (synced; edit that file) ---\n${extra}`;

      if (current === expected) return { name, detail: 'up to date' };
      if (!dryRun) writeFileSync(path, expected);

      const verb = current === undefined ? 'created' : 'updated';
      return { name, detail: dryRun ? `would be ${verb}` : verb, changed: true };
    });

    const changed = report({
      action: check ? 'check .vscodeignore' : 'sync .vscodeignore',
      dryRun: dryRun && !check,
      rows,
      write,
    });

    if (check && changed > 0) {
      write('Run `pnpm sync-vscodeignore` to update them.');
      return 1;
    }
    return 0;
  },
};

/**
 * @param {string} path
 * @returns {string | undefined} File contents, or undefined if it does not exist.
 */
function readFileIfExists(path) {
  try {
    return readFileSync(path, 'utf8');
  } catch (error) {
    if (isErrnoException(error) && error.code === 'ENOENT') return undefined;
    throw error;
  }
}

/**
 * @param {unknown} error
 * @returns {error is NodeJS.ErrnoException}
 */
function isErrnoException(error) {
  return error instanceof Error && 'code' in error;
}
