// A VSIX is a plain zip, and both the injection and the verification need to
// reach inside one. The `zip`/`unzip` binaries do the work — they are present
// wherever this packages — and the calls live here so no command spells out an
// argument list of its own.

import { execFileSync } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

/**
 * Run `fn` against a fresh temporary directory, removing it either way.
 *
 * @template T
 * @param {string} prefix Name prefix, to make a stray directory recognizable.
 * @param {(dir: string) => T} fn
 * @returns {T}
 */
export function withTempDir(prefix, fn) {
  const dir = mkdtempSync(join(tmpdir(), prefix));
  try {
    return fn(dir);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

/**
 * Add `entry` to an existing archive, recursively, resolving it against `cwd`.
 *
 * @param {string} zipPath Archive to add to; must already exist.
 * @param {string} cwd Directory the entry path is relative to.
 * @param {string} entry Top-level file or directory to add.
 * @returns {void}
 */
export function addToZip(zipPath, cwd, entry) {
  execFileSync('zip', ['-r', '-q', zipPath, entry], { cwd });
}

/**
 * @param {string} zipPath
 * @param {string} dest Directory to extract into.
 * @returns {void}
 */
export function extractZip(zipPath, dest) {
  execFileSync('unzip', ['-q', zipPath, '-d', dest]);
}

/**
 * The archive's table of contents, one entry per line.
 *
 * @param {string} zipPath
 * @returns {string} Raw `unzip -l` output.
 */
export function listZip(zipPath) {
  return execFileSync('unzip', ['-l', zipPath], { encoding: 'utf8' });
}

/**
 * Which of `entries` the archive does not contain.
 *
 * @param {string} zipPath
 * @param {readonly string[]} entries Full entry paths, as they appear in the listing.
 * @returns {string[]}
 */
export function missingFromZip(zipPath, entries) {
  const listing = listZip(zipPath);
  return entries.filter((entry) => !listing.includes(entry));
}
