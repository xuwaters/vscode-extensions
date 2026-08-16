// Editing a package.json should leave the rest of the file exactly as it was,
// so remember how it was formatted and write it back the same way.

import { readFileSync, writeFileSync } from 'node:fs';

/**
 * @template T
 * @typedef {object} JsonFile
 * @property {string} path
 * @property {T} data
 * @property {string} indent Indentation of the original file.
 * @property {boolean} trailingNewline Whether the original ended with a newline.
 */

/**
 * @param {string} text
 * @returns {string} The indentation used by the first indented line, or two spaces.
 */
export function detectIndent(text) {
  const match = /\n([ \t]+)"/.exec(text);
  return match ? match[1] : '  ';
}

/**
 * @template T
 * @param {string} path
 * @returns {JsonFile<T>}
 */
export function readJsonFile(path) {
  const raw = readFileSync(path, 'utf8');
  return {
    path,
    data: JSON.parse(raw),
    indent: detectIndent(raw),
    trailingNewline: raw.endsWith('\n'),
  };
}

/**
 * @template T
 * @param {JsonFile<T>} file
 * @returns {void}
 */
export function writeJsonFile(file) {
  const text = JSON.stringify(file.data, null, file.indent) + (file.trailingNewline ? '\n' : '');
  writeFileSync(file.path, text);
}
