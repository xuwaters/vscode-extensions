// Every extension packages the same way, so every extension wants the same
// .vscodeignore. This copies one template over all of them.

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

      if (current === template) return { name, detail: 'up to date' };
      if (!dryRun) writeFileSync(path, template);

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
