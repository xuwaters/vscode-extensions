import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import { afterEach, describe, expect, it } from 'vitest';
import { FontIndex, systemFontDirs, walk } from './fonts.js';

/** A stand-in for `TypstServer.indexFont`, so these tests need no WASM. */
function fakeParse(calls: string[]) {
  return (data: Uint8Array) => {
    const text = Buffer.from(data).toString();
    calls.push(text);
    return [{ info: { family: text }, index: 0 }];
  };
}

describe('the font index', () => {
  const dirs: string[] = [];

  const scratch = () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'typst-fonts-'));
    dirs.push(dir);
    return dir;
  };

  afterEach(() => {
    for (const dir of dirs.splice(0)) {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  it('finds font files recursively, in a stable order', () => {
    const dir = scratch();
    fs.mkdirSync(path.join(dir, 'nested'));
    fs.writeFileSync(path.join(dir, 'b.ttf'), 'b');
    fs.writeFileSync(path.join(dir, 'a.otf'), 'a');
    fs.writeFileSync(path.join(dir, 'notes.txt'), 'ignored');
    fs.writeFileSync(path.join(dir, 'nested', 'c.ttc'), 'c');

    const found = [...walk(dir)].map((file) => path.basename(file));
    expect(found).toEqual(['a.otf', 'b.ttf', 'c.ttc']);
  });

  it('parses each file once and serves the rest from the cache', () => {
    const dir = scratch();
    const cache = path.join(scratch(), 'index.json');
    fs.writeFileSync(path.join(dir, 'one.ttf'), 'one');
    fs.writeFileSync(path.join(dir, 'two.ttf'), 'two');

    const calls: string[] = [];
    const first = new FontIndex(cache, fakeParse(calls));
    const firstStats = first.addDirectories([dir]);
    first.save();

    expect(firstStats.parsed).toBe(2);
    expect(firstStats.faces).toBe(2);
    expect(calls).toEqual(['one', 'two']);

    // A second index over the same unchanged files must not re-parse anything.
    calls.length = 0;
    const second = new FontIndex(cache, fakeParse(calls));
    const secondStats = second.addDirectories([dir]);

    expect(secondStats.parsed).toBe(0);
    expect(secondStats.faces).toBe(2);
    expect(calls).toEqual([]);
  });

  it('re-parses a file whose contents changed', () => {
    const dir = scratch();
    const cache = path.join(scratch(), 'index.json');
    const file = path.join(dir, 'one.ttf');
    fs.writeFileSync(file, 'one');

    const calls: string[] = [];
    const first = new FontIndex(cache, fakeParse(calls));
    first.addDirectories([dir]);
    first.save();

    // Same path, different size and mtime — the cache key must notice.
    fs.writeFileSync(file, 'one-but-longer');

    calls.length = 0;
    const second = new FontIndex(cache, fakeParse(calls));
    second.addDirectories([dir]);
    expect(calls).toEqual(['one-but-longer']);
  });

  it('survives a corrupt cache file', () => {
    const dir = scratch();
    const cache = path.join(scratch(), 'index.json');
    fs.writeFileSync(cache, 'not json at all');
    fs.writeFileSync(path.join(dir, 'one.ttf'), 'one');

    const calls: string[] = [];
    const index = new FontIndex(cache, fakeParse(calls));
    expect(index.addDirectories([dir]).faces).toBe(1);
  });

  it('reads face bytes on demand rather than holding them', () => {
    const dir = scratch();
    fs.writeFileSync(path.join(dir, 'one.ttf'), 'bytes');

    const index = new FontIndex('', fakeParse([]));
    index.addDirectories([dir]);

    expect(Buffer.from(index.data(0)!).toString()).toBe('bytes');
    expect(index.data(99)).toBeNull();
  });

  it('sends metadata across the boundary without file paths', () => {
    const dir = scratch();
    fs.writeFileSync(path.join(dir, 'one.ttf'), 'one');

    const index = new FontIndex('', fakeParse([]));
    index.addDirectories([dir]);

    expect(index.descriptors).toEqual([{ info: { family: 'one' }, index: 0 }]);
  });

  it('only offers font directories that exist on this machine', () => {
    for (const dir of systemFontDirs()) {
      expect(fs.statSync(dir).isDirectory()).toBe(true);
    }
  });
});

/**
 * P1-11's research debt: what does indexing the bundled fonts cost at startup?
 *
 * The spike folded font parsing into a cold-start number and never isolated it,
 * which mattered because the "server start < 400 ms" target has to fit it.
 * Needs the WASM artifact, because the cost being measured is upstream's
 * `FontInfo` parser, not ours.
 */
describe.skipIf(
  !fs.existsSync(path.join(__dirname, '..', 'wasm', 'typst_lsp_wasm.js')),
)('bundled font indexing cost', () => {
  it('is a small fraction of the server-start budget, and near-free when cached', () => {
    // eslint-disable-next-line @typescript-eslint/no-require-imports
    const wasm = require(path.join(__dirname, '..', 'wasm', 'typst_lsp_wasm.js'));
    const parse = (data: Uint8Array) => wasm.TypstServer.indexFont(data);
    const bundled = path.join(__dirname, '..', 'assets', 'fonts');

    const cacheDir = fs.mkdtempSync(path.join(os.tmpdir(), 'typst-fontcache-'));
    const cachePath = path.join(cacheDir, 'font-index.json');

    try {
      const cold = new FontIndex(cachePath, parse);
      const coldStats = cold.addDirectories([bundled]);
      cold.save();

      const warm = new FontIndex(cachePath, parse);
      const warmStats = warm.addDirectories([bundled]);

      console.log(
        `\nbundled fonts: ${coldStats.faces} faces\n` +
          `  cold (parse ${coldStats.parsed} files) ${coldStats.ms} ms\n` +
          `  cached                                 ${warmStats.ms} ms\n`,
      );

      expect(coldStats.faces).toBeGreaterThanOrEqual(17);
      expect(warmStats.faces).toBe(coldStats.faces);
      expect(warmStats.parsed).toBe(0);

      // The whole server-start budget is 400 ms; fonts must not be most of it.
      expect(coldStats.ms).toBeLessThan(200);
      expect(warmStats.ms).toBeLessThanOrEqual(coldStats.ms);
    } finally {
      fs.rmSync(cacheDir, { recursive: true, force: true });
    }
  }, 60_000);
});
