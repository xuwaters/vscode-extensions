// Publish extensions to the VS Code Marketplace from the .vsix their `package`
// script built. Uploading that file as-is, rather than letting vsce repackage,
// keeps each extension's packaging flags and post-processing (fast-element-ultra
// patches its .vsix after `vsce package`) in what users install.

import { spawn } from 'node:child_process';
import { existsSync } from 'node:fs';

import { UsageError } from '../lib/cli.mjs';
import { readJsonFile } from '../lib/json-file.mjs';
import { toNameList } from '../lib/repo.mjs';
import { formatRows, plural } from '../lib/report.mjs';

/**
 * Runs a program to completion with its output shown, resolving to its exit
 * code. Injected so tests can record the calls instead of making them.
 *
 * @typedef {(command: string, args: string[], options: { cwd: string }) => Promise<number>} Exec
 */

/**
 * One extension to publish.
 *
 * @typedef {object} Target
 * @property {string} name Extension directory name.
 * @property {string} packageName `name` from its package.json.
 * @property {string} version
 * @property {string} vsixFile File name of the .vsix `vsce package` writes for this version.
 */

/** @type {Exec} */
function spawnInherit(command, args, { cwd }) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { cwd, stdio: 'inherit' });
    child.on('error', reject);
    child.on('close', (code, signal) => resolve(code ?? (signal ? 1 : 0)));
  });
}

/**
 * @param {Exec} [exec]
 * @returns {import('../lib/cli.mjs').Command<{ repo: import('../lib/repo.mjs').Repo }>}
 */
export function createPublishCommand(exec = spawnInherit) {
  return {
    name: 'publish',
    summary: 'Publish extensions to the VS Code Marketplace',

    usage: ['repo publish [--filter <name,...>] [--build] [--azure-credential] [--dry-run]'],

    options: {
      filter: {
        type: 'string',
        short: 'f',
        multiple: true,
        placeholder: '<name,...>',
        describe: 'Extension directory names to publish. Repeatable. Default: all.',
      },
      build: {
        type: 'boolean',
        short: 'b',
        describe: 'Run each extension\'s `package` script first, instead of using the .vsix on disk.',
      },
      'azure-credential': {
        type: 'boolean',
        describe: 'Sign in with Microsoft Entra ID (`az login`) instead of a personal access token.',
      },
      'dry-run': {
        type: 'boolean',
        short: 'n',
        describe: 'Print what would be built and published without running anything.',
      },
    },

    details: [
      'Each extension publishes extensions/<name>/<package-name>-<version>.vsix, the file its',
      '`package` script writes for the version in package.json. Every .vsix is checked before',
      'anything is uploaded, and a version already on the Marketplace is skipped, so bump the',
      'versions that changed (`pnpm bump -f <name>`) and publish the rest unchanged.',
      '',
      'vsce finds the token itself: the VSCE_PAT environment variable, else the publisher saved',
      'by `pnpm --filter <package-name> exec vsce login <publisher>`, else it prompts. The token',
      'needs the Marketplace (Manage) scope for all accessible organizations.',
    ],

    async run({ values, repo, write }) {
      const build = values.build === true;
      const dryRun = values['dry-run'] === true;
      const targets = repo.resolveTargets(toNameList(values.filter)).map((name) => readTarget(repo, name));
      const prefix = dryRun ? '[dry-run] ' : '';

      if (build) {
        for (const target of targets) {
          const args = ['--filter', target.packageName, 'run', 'package'];
          write(`${prefix}pnpm ${args.join(' ')}`);
          if (dryRun) continue;
          const code = await exec('pnpm', args, { cwd: repo.root });
          if (code !== 0) {
            write(`Building ${target.name} failed (exit ${code}); nothing was published.`);
            return code;
          }
        }
      }

      // A dry run with --build predicts files the build would write, so only
      // insist on them when they should already be there.
      const missing = targets.filter((t) => !existsSync(repo.path(t.name, t.vsixFile)));
      if (missing.length > 0 && !(dryRun && build)) {
        throw new UsageError(
          `Missing .vsix for ${plural(missing.length, 'extension')}; nothing was published:\n` +
            formatRows(missing.map((t) => ({ name: t.name, detail: t.vsixFile }))).join('\n') +
            '\nBuild them with --build, or `pnpm --filter <package-name> package`.',
        );
      }

      /** @type {import('../lib/report.mjs').ReportRow[]} */
      const done = [];
      for (const target of targets) {
        const args = publishArgs(target, values['azure-credential'] === true);
        write(`${prefix}(extensions/${target.name}) pnpm ${args.join(' ')}`);
        if (dryRun) continue;
        const code = await exec('pnpm', args, { cwd: repo.path(target.name) });
        if (code !== 0) {
          write(`Publishing ${target.name} failed (exit ${code}).`);
          summarize(done, targets.length, write);
          return code;
        }
        done.push({ name: target.name, detail: target.version, changed: true });
      }

      if (!dryRun) summarize(done, targets.length, write);
      return 0;
    },
  };
}

export const publishCommand = createPublishCommand();

/**
 * @param {import('../lib/repo.mjs').Repo} repo
 * @param {string} name
 * @returns {Target}
 */
function readTarget(repo, name) {
  const file = /** @type {import('../lib/json-file.mjs').JsonFile<{ name?: unknown, version?: unknown }>} */ (
    readJsonFile(repo.path(name, 'package.json'))
  );
  const { name: packageName, version } = file.data;
  if (typeof packageName !== 'string' || typeof version !== 'string') {
    throw new UsageError(`${file.path} needs string 'name' and 'version' fields`);
  }
  return { name, packageName, version, vsixFile: `${packageName}-${version}.vsix` };
}

/**
 * @param {Target} target
 * @param {boolean} azureCredential
 * @returns {string[]}
 */
export function publishArgs(target, azureCredential) {
  return [
    'exec', 'vsce', 'publish',
    '--packagePath', target.vsixFile,
    '--skip-duplicate',
    ...(azureCredential ? ['--azure-credential'] : []),
  ];
}

/**
 * vsce reports a skipped duplicate as a success, so a row here means "the
 * Marketplace now has this version", whether this run uploaded it or not.
 *
 * @param {import('../lib/report.mjs').ReportRow[]} done
 * @param {number} total
 * @param {(message: string) => void} write
 * @returns {void}
 */
function summarize(done, total, write) {
  write(`On the Marketplace (${plural(done.length, 'extension')} of ${total}):`);
  for (const line of formatRows(done)) write(line);
}
