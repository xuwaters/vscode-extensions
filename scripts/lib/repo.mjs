// The repository as the scripts see it: where it is, and which extensions live
// in it. Every path goes through a `Repo`, so tests can point the same commands
// at a temporary directory.

import { readdirSync, statSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { UsageError } from './cli.mjs';

/** Repo root inferred from this file's location: scripts/lib/repo.mjs -> ../.. */
export const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');

/**
 * @typedef {object} Repo
 * @property {string} root
 * @property {string} extensionsDir
 * @property {() => string[]} listExtensions Directory names, sorted, each holding a package.json.
 * @property {(name: string, ...parts: string[]) => string} path Path inside an extension.
 * @property {(filters: string[]) => string[]} resolveTargets Extensions matching `filters`.
 */

/**
 * @param {string} [root] Repository root; defaults to this checkout.
 * @returns {Repo}
 */
export function createRepo(root = REPO_ROOT) {
  const extensionsDir = join(root, 'extensions');

  /** @returns {string[]} */
  function listExtensions() {
    let entries;
    try {
      entries = readdirSync(extensionsDir, { withFileTypes: true });
    } catch {
      throw new UsageError(`No extensions directory at ${extensionsDir}`);
    }
    return entries
      .filter((entry) => entry.isDirectory() && isFile(join(extensionsDir, entry.name, 'package.json')))
      .map((entry) => entry.name)
      .sort();
  }

  /**
   * @param {string[]} filters Extension directory names; empty means all of them.
   * @returns {string[]}
   */
  function resolveTargets(filters) {
    const all = listExtensions();
    if (filters.length === 0) return all;

    const unknown = filters.filter((filter) => !all.includes(filter));
    if (unknown.length > 0) {
      throw new UsageError(
        `Unknown extension${unknown.length === 1 ? '' : 's'}: ${unknown.join(', ')}.\n` +
          `Available: ${all.join(', ')}`,
      );
    }
    return all.filter((name) => filters.includes(name));
  }

  return {
    root,
    extensionsDir,
    listExtensions,
    resolveTargets,
    path: (name, ...parts) => join(extensionsDir, name, ...parts),
  };
}

/**
 * @param {string} path
 * @returns {boolean}
 */
function isFile(path) {
  try {
    return statSync(path).isFile();
  } catch {
    return false;
  }
}

/**
 * Flatten repeatable, comma-separated flags into a list of names:
 * `-f a,b -f c` becomes `['a', 'b', 'c']`.
 *
 * @param {import('./cli.mjs').ParsedValues[string]} value
 * @returns {string[]}
 */
export function toNameList(value) {
  if (value === undefined || typeof value === 'boolean') return [];
  const values = Array.isArray(value) ? value : [value];
  return values
    .filter((entry) => typeof entry === 'string')
    .flatMap((entry) => entry.split(',').map((s) => s.trim()))
    .filter(Boolean);
}
