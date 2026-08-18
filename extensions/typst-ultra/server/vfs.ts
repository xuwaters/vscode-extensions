import * as fs from 'fs';
import * as path from 'path';

/**
 * Where a virtual root maps to on this machine.
 *
 * The Rust side describes a root as either `''` (the project) or a package spec
 * like `@preview/cetz:0.4.2`. Resolving those is the host's job because only it
 * knows where the project is and where typst keeps its package cache.
 */
export interface Roots {
  /** The compile root, absolute. */
  project: string;
  /** The package cache directory, absolute. Empty disables package reads. */
  packageCache: string;
}

/** `@namespace/name:version`, as the Rust side spells it. */
const SPEC = /^@([^/]+)\/([^:]+):(.+)$/;

/**
 * Resolve a root descriptor plus a virtual path to a real file path.
 *
 * Returns null when the path escapes its root. `VirtualPath` normalises `..`
 * before it reaches us, so this should never fire — which is exactly why it is
 * worth checking: a confinement bug is not something to find out about from a
 * bug report.
 */
export function resolve(
  roots: Roots,
  root: string,
  vpath: string,
): string | null {
  const base = baseOf(roots, root);
  if (!base) return null;

  // The Rust side sends `/a/b.typ`; strip the leading slash so it joins as a
  // relative path rather than resolving to the filesystem root.
  const relative = vpath.replace(/^[/\\]+/, '');
  const full = path.resolve(base, relative);

  return isInside(base, full) ? full : null;
}

/** The real directory a root descriptor names. */
export function baseOf(roots: Roots, root: string): string | null {
  if (root === '') return roots.project;
  if (!roots.packageCache) return null;

  const match = SPEC.exec(root);
  if (!match) return null;
  const [, namespace, name, version] = match;

  // Reject anything that could climb out of the cache directory. Package specs
  // come from document text, so they are attacker-influenced.
  for (const part of [namespace, name, version]) {
    if (!part || part.includes('/') || part.includes('\\') || part === '..') {
      return null;
    }
  }

  return path.join(roots.packageCache, namespace, name, version);
}

/** Whether `candidate` is `base` itself or below it. */
export function isInside(base: string, candidate: string): boolean {
  const relative = path.relative(base, candidate);
  return (
    relative === '' ||
    (!relative.startsWith('..') && !path.isAbsolute(relative))
  );
}

/** Read a file, or null if it is not there. */
export function readFile(
  roots: Roots,
  root: string,
  vpath: string,
): Uint8Array | null {
  const full = resolve(roots, root, vpath);
  if (!full) return null;
  try {
    return new Uint8Array(fs.readFileSync(full));
  } catch {
    return null;
  }
}

/** List a directory's entries, for path completions. */
export function listDir(roots: Roots, root: string, vpath: string): string[] {
  const full = resolve(roots, root, vpath);
  if (!full) return [];
  try {
    return fs.readdirSync(full);
  } catch {
    return [];
  }
}
