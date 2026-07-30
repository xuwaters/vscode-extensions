//! Path helpers. Every path handled by the walker comes from `Uri.path`, so it
//! is absolute and slash-separated on every platform.

/** Last segment of `path`, or the empty string for the filesystem root. */
export function baseName(path: string): string {
  const i = path.lastIndexOf('/');
  return i < 0 ? path : path.slice(i + 1);
}

/** `path` with `name` appended as a child segment. */
export function joinPath(path: string, name: string): string {
  return path.endsWith('/') ? `${path}${name}` : `${path}/${name}`;
}

/** `path` relative to `base`, or `undefined` when it is not under `base`. */
export function relativeTo(base: string, path: string): string | undefined {
  if (path === base) return '';
  const prefix = base.endsWith('/') ? base : `${base}/`;
  return path.startsWith(prefix) ? path.slice(prefix.length) : undefined;
}
