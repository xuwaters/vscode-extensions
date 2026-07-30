//! The walk itself: descend each root, then remove empty target directories on
//! the way back up.

import { IgnoreStack } from './gitignore.js';
import { baseName } from './paths.js';
import type { Entry, Vfs } from './vfs.js';

/** What to look for and what to do with it. */
export interface Config {
  /** Directory names removed when empty. */
  names: string[];
  /** Directory names entered even when .gitignore excludes them. */
  descend: string[];
  /** Directory names removed once the scan leaves them empty. */
  prune: string[];
  dryRun: boolean;
  noIgnore: boolean;
}

/** Where a run reports to. A silent run passes a reporter that drops both. */
export interface Reporter {
  say(message: string): void;
  error(message: string): void;
}

/** Anything shaped like a `vscode.CancellationToken`. */
export interface Cancellable {
  readonly isCancellationRequested: boolean;
}

/** What a run added up to. */
export interface Summary {
  removed: number;
  errors: number;
  /** Directories removed — under `dryRun`, reported as removable — in walk order. */
  paths: string[];
  cancelled: boolean;
}

export class Cleaner {
  private removed = 0;
  private errors = 0;
  private readonly removedPaths: string[] = [];
  /**
   * Dry-run only: directories reported as removable. They still exist on disk,
   * so a parent only looks empty once they are discounted.
   */
  private readonly reported = new Set<string>();

  constructor(
    private readonly cfg: Config,
    private readonly fs: Vfs,
    private readonly reporter: Reporter,
    private readonly token?: Cancellable,
  ) {}

  /** Scan each root and report the totals. */
  async run(roots: string[]): Promise<Summary> {
    for (const root of roots) {
      if (this.cancelled) break;
      await this.scan(root);
    }

    return {
      removed: this.removed,
      errors: this.errors,
      paths: [...this.removedPaths],
      cancelled: this.cancelled,
    };
  }

  private get cancelled(): boolean {
    return this.token?.isCancellationRequested ?? false;
  }

  private async scan(root: string): Promise<void> {
    try {
      if ((await this.fs.kind(root)) !== 'dir') {
        this.error(`${root}: not a directory`);
        return;
      }
    } catch (e) {
      this.error(`${root}: ${message(e)}`);
      return;
    }

    const ignores = new IgnoreStack();
    await this.visit(root, ignores);
    // The root itself is a candidate when its name matches.
    if (!this.cancelled && this.isTarget(root)) await this.tryRemove(root);
  }

  /**
   * Walk `dir` depth-first, removing empty target directories on the way back
   * up so nested matches collapse in one pass.
   */
  private async visit(dir: string, ignores: IgnoreStack): Promise<void> {
    if (this.cancelled) return;

    if (!this.cfg.noIgnore) await ignores.push(dir, this.fs);

    const entries = await this.list(dir);
    for (const entry of entries ?? []) {
      if (this.cancelled) break;
      // Never follow symlinks: a symlinked directory is not a directory as far
      // as the walk is concerned.
      if (entry.kind !== 'dir' || entry.name === '.git') continue;

      const target = this.isTarget(entry.path);
      if (
        !this.cfg.noIgnore &&
        !target &&
        !this.isDescend(entry.path) &&
        ignores.isIgnored(entry.path, true)
      ) {
        continue;
      }

      await this.visit(entry.path, ignores);
      if (target) await this.tryRemove(entry.path);
    }

    if (!this.cfg.noIgnore) ignores.pop();

    // Targets are removed by the caller, which owns the root case too. A
    // directory that could not be listed is never shown to be empty, so it is
    // left alone rather than reported a second time.
    if (entries && !this.cancelled && this.isPrune(dir) && !this.isTarget(dir)) {
      await this.tryRemove(dir);
    }
  }

  /** Listing of `dir`, or `undefined` when it could not be read. */
  private async list(dir: string): Promise<Entry[] | undefined> {
    try {
      const entries = await this.fs.readDir(dir);
      // Listing order is filesystem-dependent; sort so output is stable.
      return entries.sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
    } catch (e) {
      this.error(`${dir}: ${message(e)}`);
      return undefined;
    }
  }

  private async tryRemove(path: string): Promise<void> {
    try {
      if (!(await this.isEmptyDir(path))) return;
    } catch (e) {
      this.error(`${path}: ${message(e)}`);
      return;
    }

    if (this.cfg.dryRun) {
      this.count(path);
      this.reported.add(path);
      this.say(`would remove ${path}`);
      return;
    }

    try {
      await this.fs.removeDir(path);
      this.count(path);
      this.say(`removed ${path}`);
    } catch (e) {
      this.error(`${path}: ${message(e)}`);
    }
  }

  /**
   * Under a dry run nothing is unlinked, so entries already reported as
   * removable are discounted; otherwise a parent would never look empty.
   */
  private async isEmptyDir(path: string): Promise<boolean> {
    const entries = await this.fs.readDir(path);
    return entries.every(entry => this.cfg.dryRun && this.reported.has(entry.path));
  }

  private isTarget(path: string): boolean {
    return nameMatches(path, this.cfg.names);
  }

  /**
   * Directories that stay on the walk even when .gitignore excludes them:
   * `.claude` is typically ignored, yet holds the `.cc-writes` targets.
   */
  private isDescend(path: string): boolean {
    return nameMatches(path, this.cfg.descend);
  }

  /**
   * Containers that follow their contents out: an emptied `.claude` is left
   * behind by removing the `.cc-writes` it held.
   */
  private isPrune(path: string): boolean {
    return nameMatches(path, this.cfg.prune);
  }

  private count(path: string): void {
    this.removed++;
    this.removedPaths.push(path);
  }

  private say(text: string): void {
    this.reporter.say(text);
  }

  private error(text: string): void {
    this.errors++;
    this.reporter.error(text);
  }
}

function nameMatches(path: string, names: string[]): boolean {
  const name = baseName(path);
  return name !== '' && names.includes(name);
}

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/** "1 directory" / "2 directories". */
export function directories(n: number): string {
  return n === 1 ? '1 directory' : `${n} directories`;
}
