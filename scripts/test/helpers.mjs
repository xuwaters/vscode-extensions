// Shared test scaffolding: a throwaway repo on disk and a writer that records
// output instead of printing it.

import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { after } from 'node:test';

import { createRepo } from '../lib/repo.mjs';

/**
 * A temporary repo with the given extensions, removed when the test file ends.
 *
 * @param {Record<string, { version?: string, vscodeignore?: string }>} extensions
 * @returns {import('../lib/repo.mjs').Repo}
 */
export function makeRepo(extensions) {
  const root = mkdtempSync(join(tmpdir(), 'repo-scripts-test-'));
  after(() => rmSync(root, { recursive: true, force: true }));

  for (const [name, { version = '1.0.0', vscodeignore }] of Object.entries(extensions)) {
    const dir = join(root, 'extensions', name);
    mkdirSync(dir, { recursive: true });
    writeFileSync(join(dir, 'package.json'), `${JSON.stringify({ name, version }, null, 2)}\n`);
    if (vscodeignore !== undefined) writeFileSync(join(dir, '.vscodeignore'), vscodeignore);
  }

  return createRepo(root);
}

/**
 * @param {import('../lib/repo.mjs').Repo} repo
 * @param {string} name
 * @param {string} file
 * @returns {string}
 */
export function readExtensionFile(repo, name, file) {
  return readFileSync(repo.path(name, file), 'utf8');
}

/**
 * @returns {{ write: (message: string) => void, lines: string[], text: () => string }}
 */
export function makeWriter() {
  /** @type {string[]} */
  const lines = [];
  return {
    write: (message) => lines.push(message),
    lines,
    text: () => lines.join('\n'),
  };
}
