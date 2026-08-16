// Version bumps for extensions/*/package.json.

import { UsageError } from '../lib/cli.mjs';
import { readJsonFile, writeJsonFile } from '../lib/json-file.mjs';
import { toNameList } from '../lib/repo.mjs';
import { report } from '../lib/report.mjs';
import { bumpSemver, isSemver } from '../lib/semver.mjs';

/** @type {import('../lib/semver.mjs').ReleaseType[]} */
const RELEASES = ['major', 'minor', 'patch'];

/** @type {import('../lib/cli.mjs').Command} */
export const bumpCommand = {
  name: 'bump',
  summary: 'Bump the version in extensions/*/package.json',

  usage: [
    'repo bump [--patch|--minor|--major] [--filter <name,...>] [--dry-run]',
    'repo bump --set <x.y.z> [--filter <name,...>] [--dry-run]',
  ],

  options: {
    patch: { type: 'boolean', describe: 'Increment patch (default).' },
    minor: { type: 'boolean', describe: 'Increment minor, reset patch.' },
    major: { type: 'boolean', describe: 'Increment major, reset minor and patch.' },
    set: {
      type: 'string',
      placeholder: '<x.y.z>',
      describe: 'Set every targeted extension to this exact version.',
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
      describe: 'Extension directory names to bump. Repeatable. Default: all.',
    },
  },

  run({ values, repo, write }) {
    const release = resolveRelease(values);
    const setVersion = resolveSetVersion(values);
    const dryRun = values['dry-run'] === true;
    const targets = repo.resolveTargets(toNameList(values.filter));

    /** @type {import('../lib/report.mjs').ReportRow[]} */
    const rows = targets.map((name) => {
      const file = /** @type {import('../lib/json-file.mjs').JsonFile<{ version?: string }>} */ (
        readJsonFile(repo.path(name, 'package.json'))
      );
      const current = file.data.version;
      if (typeof current !== 'string') {
        throw new UsageError(`${file.path} has no string 'version' field`);
      }

      const next = setVersion ?? bumpSemver(current, release);
      if (current === next) return { name, detail: `${current} (unchanged)` };

      if (!dryRun) {
        file.data.version = next;
        writeJsonFile(file);
      }
      return { name, detail: `${current} -> ${next}`, changed: true };
    });

    report({
      action: setVersion ? `set version ${setVersion}` : `bump ${release}`,
      dryRun,
      rows,
      write,
    });
    return 0;
  },
};

/**
 * @param {import('../lib/cli.mjs').ParsedValues} values
 * @returns {import('../lib/semver.mjs').ReleaseType}
 */
function resolveRelease(values) {
  const chosen = RELEASES.filter((release) => values[release] === true);
  if (chosen.length > 1) {
    throw new UsageError(`Specify at most one of ${RELEASES.map((r) => `--${r}`).join(', ')}`);
  }
  return chosen[0] ?? 'patch';
}

/**
 * @param {import('../lib/cli.mjs').ParsedValues} values
 * @returns {string | undefined}
 */
function resolveSetVersion(values) {
  const set = values.set;
  if (set === undefined) return undefined;
  if (typeof set !== 'string' || !isSemver(set)) {
    throw new UsageError(`--set value '${String(set)}' is not a valid semver`);
  }
  return set;
}
