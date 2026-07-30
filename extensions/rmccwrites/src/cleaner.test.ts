import { describe, expect, it } from 'vitest';
import { Cleaner, directories, type Cancellable, type Config, type Summary } from './cleaner.js';
import { MemFs } from './memFs.js';

/** Defaults matching the shipped settings, so tests only state what they change. */
function config(overrides: Partial<Config> = {}): Config {
  return {
    names: ['.cc-writes'],
    descend: ['.claude'],
    prune: ['.claude'],
    dryRun: false,
    noIgnore: false,
    ...overrides,
  };
}

interface Run {
  out: string[];
  err: string[];
  summary: Summary;
}

async function clean(fs: MemFs, cfg: Config, roots: string[], token?: Cancellable): Promise<Run> {
  const out: string[] = [];
  const err: string[] = [];
  const reporter = { say: (m: string) => out.push(m), error: (m: string) => err.push(m) };
  const summary = await new Cleaner(cfg, fs, reporter, token).run(roots);
  return { out, err, summary };
}

describe('Cleaner', () => {
  it('removes an empty target and reports it', async () => {
    const fs = new MemFs();
    fs.dir('/w/a/.cc-writes');
    fs.file('/w/a/keep.txt', '');

    const run = await clean(fs, config(), ['/w']);

    expect(fs.exists('/w/a/.cc-writes')).toBe(false);
    expect(fs.exists('/w/a')).toBe(true);
    expect(run.out).toEqual(['removed /w/a/.cc-writes']);
    expect(run.summary).toMatchObject({ removed: 1, errors: 0, paths: ['/w/a/.cc-writes'], cancelled: false });
  });

  it('leaves a non-empty target alone', async () => {
    const fs = new MemFs();
    fs.file('/w/.cc-writes/note.md', 'hi');

    const run = await clean(fs, config(), ['/w']);

    expect(fs.exists('/w/.cc-writes')).toBe(true);
    expect(run.out).toEqual([]);
    expect(run.summary).toMatchObject({ removed: 0, errors: 0 });
  });

  it('collapses nested targets bottom-up', async () => {
    const fs = new MemFs();
    fs.dir('/w/.cc-writes/.cc-writes');

    const run = await clean(fs, config(), ['/w']);

    expect(fs.paths()).toEqual(['/w']);
    expect(run.out).toEqual(['removed /w/.cc-writes/.cc-writes', 'removed /w/.cc-writes']);
  });

  it('treats a root that is itself a target as a candidate', async () => {
    const fs = new MemFs();
    fs.dir('/w/.cc-writes');

    const run = await clean(fs, config(), ['/w/.cc-writes']);

    expect(fs.exists('/w/.cc-writes')).toBe(false);
    expect(run.summary.removed).toBe(1);
  });

  it('scans several roots in order', async () => {
    const fs = new MemFs();
    fs.dir('/a/.cc-writes');
    fs.dir('/b/.cc-writes');

    const run = await clean(fs, config(), ['/a', '/b']);

    expect(run.out).toEqual(['removed /a/.cc-writes', 'removed /b/.cc-writes']);
  });

  it('lets an emptied prune directory follow its target out', async () => {
    const fs = new MemFs();
    fs.dir('/w/.claude/.cc-writes');

    const run = await clean(fs, config(), ['/w']);

    expect(fs.paths()).toEqual(['/w']);
    expect(run.out).toEqual(['removed /w/.claude/.cc-writes', 'removed /w/.claude']);
  });

  it('keeps a prune directory that holds anything else', async () => {
    const fs = new MemFs();
    fs.dir('/w/.claude/.cc-writes');
    fs.file('/w/.claude/settings.json', '{}');

    const run = await clean(fs, config(), ['/w']);

    expect(fs.exists('/w/.claude')).toBe(true);
    expect(fs.exists('/w/.claude/.cc-writes')).toBe(false);
    expect(run.summary.removed).toBe(1);
  });

  it('removes a prune directory given as the root', async () => {
    const fs = new MemFs();
    fs.dir('/w/.claude/.cc-writes');

    const run = await clean(fs, config(), ['/w/.claude']);

    expect(fs.exists('/w/.claude')).toBe(false);
    expect(run.summary.removed).toBe(2);
  });

  it('removes a prune directory that was already empty', async () => {
    const fs = new MemFs();
    fs.dir('/w/.claude');

    const run = await clean(fs, config(), ['/w']);

    expect(fs.exists('/w/.claude')).toBe(false);
    expect(run.summary.removed).toBe(1);
  });

  it('prunes the walk at gitignored directories', async () => {
    const fs = new MemFs();
    fs.file('/w/.gitignore', 'build/\n');
    fs.dir('/w/build/.cc-writes');
    fs.dir('/w/src/.cc-writes');

    const run = await clean(fs, config(), ['/w']);

    expect(fs.exists('/w/build/.cc-writes')).toBe(true);
    expect(fs.exists('/w/src/.cc-writes')).toBe(false);
    expect(run.summary.removed).toBe(1);
  });

  it('scans ignored directories when gitignore is not read', async () => {
    const fs = new MemFs();
    fs.file('/w/.gitignore', 'build/\n');
    fs.dir('/w/build/.cc-writes');

    const run = await clean(fs, config({ noIgnore: true }), ['/w']);

    expect(fs.exists('/w/build/.cc-writes')).toBe(false);
    expect(run.summary.removed).toBe(1);
  });

  it('honours a nested gitignore negation', async () => {
    const fs = new MemFs();
    fs.file('/w/.gitignore', 'vendor\n');
    fs.file('/w/sub/.gitignore', '!vendor\n');
    fs.dir('/w/vendor/.cc-writes');
    fs.dir('/w/sub/vendor/.cc-writes');

    const run = await clean(fs, config(), ['/w']);

    expect(fs.exists('/w/vendor/.cc-writes')).toBe(true);
    expect(fs.exists('/w/sub/vendor/.cc-writes')).toBe(false);
    expect(run.summary.removed).toBe(1);
  });

  it('still visits ignored target and descend directories', async () => {
    const fs = new MemFs();
    fs.file('/w/.gitignore', '.claude/\n.cc-writes/\n');
    fs.dir('/w/.claude/.cc-writes');
    fs.dir('/w/.cc-writes');

    const run = await clean(fs, config(), ['/w']);

    expect(fs.paths()).toEqual(['/w', '/w/.gitignore']);
    expect(run.summary.removed).toBe(3);
  });

  it('applies descend only to the named directories', async () => {
    const fs = new MemFs();
    fs.file('/w/.gitignore', '.claude/\n.config/\n');
    fs.dir('/w/.claude/.cc-writes');
    fs.dir('/w/.config/.cc-writes');

    // The default descend list covers .claude but not .config.
    let run = await clean(fs, config(), ['/w']);
    expect(fs.exists('/w/.claude')).toBe(false);
    expect(fs.exists('/w/.config/.cc-writes')).toBe(true);
    expect(run.summary.removed).toBe(2);

    run = await clean(fs, config({ descend: ['.claude', '.config'] }), ['/w']);
    expect(fs.exists('/w/.config/.cc-writes')).toBe(false);
    expect(run.summary.removed).toBe(1);
  });

  it('never enters .git directories or symlinks', async () => {
    const fs = new MemFs();
    fs.dir('/w/.git/.cc-writes');
    fs.symlink('/w/link');
    fs.dir('/w/link/.cc-writes');

    const run = await clean(fs, config(), ['/w']);

    expect(fs.exists('/w/.git/.cc-writes')).toBe(true);
    expect(fs.exists('/w/link/.cc-writes')).toBe(true);
    expect(run.summary).toMatchObject({ removed: 0, errors: 0 });
  });

  it('lets custom names replace the default', async () => {
    const fs = new MemFs();
    fs.dir('/w/.cache');
    fs.dir('/w/.tmp');
    fs.dir('/w/.cc-writes');

    const run = await clean(fs, config({ names: ['.cache', '.tmp'] }), ['/w']);

    expect(fs.paths()).toEqual(['/w', '/w/.cc-writes']);
    expect(run.summary.removed).toBe(2);
  });

  it('finds nothing when no names are configured', async () => {
    const fs = new MemFs();
    fs.dir('/w/.claude/.cc-writes');

    const run = await clean(fs, config({ names: [], prune: [] }), ['/w']);

    expect(fs.exists('/w/.claude/.cc-writes')).toBe(true);
    expect(run.summary).toMatchObject({ removed: 0, errors: 0 });
  });

  it('removes nothing on a dry run but still counts the parent', async () => {
    const fs = new MemFs();
    fs.dir('/w/.claude/.cc-writes');

    const run = await clean(fs, config({ dryRun: true }), ['/w']);

    expect(fs.paths()).toEqual(['/w', '/w/.claude', '/w/.claude/.cc-writes']);
    expect(run.out).toEqual([
      'would remove /w/.claude/.cc-writes',
      // .claude looks empty once the target it holds is discounted.
      'would remove /w/.claude',
    ]);
    expect(run.summary.paths).toEqual(['/w/.claude/.cc-writes', '/w/.claude']);
  });

  it('reports an unreadable directory and carries on', async () => {
    const fs = new MemFs();
    fs.dir('/w/locked/.cc-writes');
    fs.dir('/w/open/.cc-writes');
    fs.unreadable('/w/locked');

    const run = await clean(fs, config(), ['/w']);

    expect(run.err).toEqual(['/w/locked: permission denied']);
    expect(fs.exists('/w/locked/.cc-writes')).toBe(true);
    expect(fs.exists('/w/open/.cc-writes')).toBe(false);
    expect(run.summary).toMatchObject({ removed: 1, errors: 1 });
  });

  it('reports a bad root without stopping the rest', async () => {
    const fs = new MemFs();
    fs.file('/w/file.txt', '');
    fs.dir('/w/.cc-writes');

    const run = await clean(fs, config(), ['/missing', '/w/file.txt', '/w']);

    expect(run.summary).toMatchObject({ removed: 1, errors: 2 });
    expect(run.err).toEqual(['/missing: no such file or directory', '/w/file.txt: not a directory']);
  });

  it('stops as soon as the token is cancelled', async () => {
    const fs = new MemFs();
    fs.dir('/a/.cc-writes');
    fs.dir('/b/.cc-writes');

    const out: string[] = [];
    const token = {
      get isCancellationRequested(): boolean {
        return out.length > 0;
      },
    };
    const reporter = { say: (m: string) => out.push(m), error: () => {} };
    const summary = await new Cleaner(config(), fs, reporter, token).run(['/a', '/b']);

    expect(summary).toMatchObject({ removed: 1, cancelled: true });
    expect(fs.exists('/b/.cc-writes')).toBe(true);
  });
});

describe('directories', () => {
  it('is singular for one', () => {
    expect(directories(0)).toBe('0 directories');
    expect(directories(1)).toBe('1 directory');
    expect(directories(2)).toBe('2 directories');
  });
});
