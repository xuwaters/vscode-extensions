//! Settings surface: the raw configuration values and the defaults they fill
//! in. Kept free of `vscode` so the normalisation can be tested directly.

import type { Config } from './cleaner.js';

export const DEFAULT_NAMES = ['.cc-writes'];
export const DEFAULT_DESCEND = ['.claude'];
export const DEFAULT_PRUNE = ['.claude'];

/** What `workspace.getConfiguration('rmccwrites')` hands back, untrusted. */
export interface RawSettings {
  names?: unknown;
  descend?: unknown;
  prune?: unknown;
  respectGitignore?: unknown;
}

export function resolveConfig(raw: RawSettings, dryRun: boolean): Config {
  return {
    names: nameList(raw.names, DEFAULT_NAMES),
    descend: nameList(raw.descend, DEFAULT_DESCEND),
    prune: nameList(raw.prune, DEFAULT_PRUNE),
    dryRun,
    noIgnore: raw.respectGitignore === false,
  };
}

/**
 * Base names from a settings array, deduplicated. `fallback` covers a missing
 * or malformed value; an explicitly empty array is left empty, since that is a
 * deliberate "none". Only base names are matched, so entries holding a `/` are
 * dropped, as is `.git`, which the walk never enters.
 */
export function nameList(value: unknown, fallback: string[]): string[] {
  if (!Array.isArray(value)) return [...fallback];
  const names = value
    .filter((v): v is string => typeof v === 'string')
    .map(v => v.trim())
    .filter(v => v !== '' && v !== '.' && v !== '..' && v !== '.git' && !v.includes('/'));
  return [...new Set(names)];
}
