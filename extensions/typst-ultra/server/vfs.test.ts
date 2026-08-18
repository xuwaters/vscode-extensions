import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { baseOf, isInside, listDir, readFile, resolve, type Roots } from './vfs.js';

describe('VFS path confinement', () => {
  let workspace: string;
  let cache: string;
  let roots: Roots;

  beforeAll(() => {
    workspace = fs.mkdtempSync(path.join(os.tmpdir(), 'typst-vfs-'));
    cache = path.join(workspace, 'cache');
    fs.mkdirSync(path.join(workspace, 'project', 'chapters'), { recursive: true });
    fs.mkdirSync(path.join(cache, 'preview', 'cetz', '0.4.2'), { recursive: true });

    fs.writeFileSync(path.join(workspace, 'project', 'main.typ'), '= Main\n');
    fs.writeFileSync(
      path.join(workspace, 'project', 'chapters', 'one.typ'),
      '= One\n',
    );
    fs.writeFileSync(path.join(workspace, 'secret.txt'), 'not for the compiler');
    fs.writeFileSync(
      path.join(cache, 'preview', 'cetz', '0.4.2', 'lib.typ'),
      '#let canvas = 1\n',
    );

    roots = { project: path.join(workspace, 'project'), packageCache: cache };
  });

  afterAll(() => {
    fs.rmSync(workspace, { recursive: true, force: true });
  });

  it('reads a project file', () => {
    const bytes = readFile(roots, '', '/main.typ');
    expect(Buffer.from(bytes!).toString()).toBe('= Main\n');
  });

  it('reads a nested project file', () => {
    const bytes = readFile(roots, '', '/chapters/one.typ');
    expect(Buffer.from(bytes!).toString()).toBe('= One\n');
  });

  it('reads a package file through the cache layout typst-cli uses', () => {
    const bytes = readFile(roots, '@preview/cetz:0.4.2', '/lib.typ');
    expect(Buffer.from(bytes!).toString()).toBe('#let canvas = 1\n');
  });

  it('refuses a path that climbs out of the project root', () => {
    expect(resolve(roots, '', '/../secret.txt')).toBeNull();
    expect(resolve(roots, '', '/chapters/../../secret.txt')).toBeNull();
    expect(readFile(roots, '', '/../secret.txt')).toBeNull();
  });

  it('refuses a package spec that tries to escape the cache', () => {
    expect(baseOf(roots, '@../../etc/passwd:1.0.0')).toBeNull();
    expect(baseOf(roots, '@preview/..:0.1.0')).toBeNull();
  });

  it('refuses a package root when no cache is configured', () => {
    const noCache: Roots = { project: roots.project, packageCache: '' };
    expect(baseOf(noCache, '@preview/cetz:0.4.2')).toBeNull();
  });

  it('returns null rather than throwing for a file that is not there', () => {
    expect(readFile(roots, '', '/nope.typ')).toBeNull();
  });

  it('lists a directory for path completions', () => {
    expect(listDir(roots, '', '/chapters')).toEqual(['one.typ']);
    expect(listDir(roots, '', '/nope')).toEqual([]);
  });

  it('knows what is inside a directory', () => {
    expect(isInside('/a/b', '/a/b/c')).toBe(true);
    expect(isInside('/a/b', '/a/b')).toBe(true);
    expect(isInside('/a/b', '/a/c')).toBe(false);
    // A sibling with a shared prefix is not inside.
    expect(isInside('/a/b', '/a/bb/c')).toBe(false);
  });
});
