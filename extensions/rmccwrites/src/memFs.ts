//! In-memory {@link Vfs} used by the tests: a flat map of paths to node kinds,
//! with every path stored absolute and slash-separated.

import { baseName } from './paths.js';
import type { Entry, Kind, Vfs } from './vfs.js';

type Node = { kind: 'dir' } | { kind: 'file'; content: string } | { kind: 'symlink' };

export class MemFs implements Vfs {
  private readonly nodes = new Map<string, Node>();
  /** Paths whose listing fails, as an unreadable directory would. */
  private readonly denied = new Set<string>();

  /**
   * Add a directory, creating its ancestors. Existing nodes are left as they
   * are, so a directory may be placed under a symlink.
   */
  dir(path: string): this {
    for (const ancestor of ancestors(path)) {
      if (!this.nodes.has(ancestor)) this.nodes.set(ancestor, { kind: 'dir' });
    }
    return this;
  }

  /** Add a file, creating its parent directories. */
  file(path: string, content: string): this {
    const parent = parentOf(path);
    if (parent !== undefined) this.dir(parent);
    this.nodes.set(path, { kind: 'file', content });
    return this;
  }

  /**
   * Add a symlink, creating its parent directories. Whatever it points at is
   * irrelevant: the walk must never traverse it.
   */
  symlink(path: string): this {
    const parent = parentOf(path);
    if (parent !== undefined) this.dir(parent);
    this.nodes.set(path, { kind: 'symlink' });
    return this;
  }

  /** Make listing `path` fail. */
  unreadable(path: string): this {
    this.denied.add(path);
    return this;
  }

  exists(path: string): boolean {
    return this.nodes.has(path);
  }

  /** Every surviving path, sorted; handy for asserting the end state. */
  paths(): string[] {
    return [...this.nodes.keys()].sort();
  }

  async kind(path: string): Promise<Kind> {
    const node = this.nodes.get(path);
    if (!node) throw new Error('no such file or directory');
    return node.kind;
  }

  async readDir(dir: string): Promise<Entry[]> {
    this.requireDir(dir);
    if (this.denied.has(dir)) throw new Error('permission denied');
    return this.children(dir).map(([path, node]) => ({
      path,
      name: baseName(path),
      kind: node.kind,
    }));
  }

  async removeDir(dir: string): Promise<void> {
    this.requireDir(dir);
    if (this.children(dir).length > 0) throw new Error('directory not empty');
    this.nodes.delete(dir);
  }

  async readText(path: string): Promise<string> {
    const node = this.nodes.get(path);
    if (!node) throw new Error('no such file or directory');
    if (node.kind !== 'file') throw new Error('not a file');
    return node.content;
  }

  private requireDir(path: string): void {
    const node = this.nodes.get(path);
    if (!node) throw new Error('no such file or directory');
    if (node.kind !== 'dir') throw new Error('not a directory');
  }

  private children(dir: string): [string, Node][] {
    return [...this.nodes.entries()]
      .filter(([path]) => parentOf(path) === dir)
      .sort(([a], [b]) => (a < b ? -1 : 1));
  }
}

/** `path` and each of its parents, stopping before the filesystem root. */
function ancestors(path: string): string[] {
  const list: string[] = [];
  for (let p: string | undefined = path; p !== undefined && p !== '/'; p = parentOf(p)) {
    list.push(p);
  }
  return list;
}

function parentOf(path: string): string | undefined {
  if (path === '/') return undefined;
  const i = path.lastIndexOf('/');
  if (i < 0) return undefined;
  return i === 0 ? '/' : path.slice(0, i);
}
