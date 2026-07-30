//! Filesystem access behind an interface, so the walker can be driven by an
//! in-memory tree in tests instead of real directories.

/**
 * What a path is, without following symlinks: a symlink to a directory is
 * `'symlink'`, never `'dir'`.
 */
export type Kind = 'dir' | 'file' | 'symlink';

/** One entry of a directory listing. */
export interface Entry {
  path: string;
  name: string;
  kind: Kind;
}

export interface Vfs {
  /** Kind of `path` itself, without following a final symlink. */
  kind(path: string): Promise<Kind>;

  /** List `dir`. Rejects when the directory cannot be opened. */
  readDir(dir: string): Promise<Entry[]>;

  /** Remove `dir`, which must be empty. */
  removeDir(dir: string): Promise<void>;

  readText(path: string): Promise<string>;
}
