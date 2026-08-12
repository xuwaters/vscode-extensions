import * as fs from 'fs';
import { createRequire } from 'module';
import * as path from 'path';
import { describe, expect, it } from 'vitest';
import { EngineSession } from './engine';
import { typedKeys } from './util';

/**
 * The TypeScript side of the WASM contract: keys in, parsed `Effects` out.
 * `wasm/` is a build artifact, so these are skipped until `pnpm run
 * build:wasm` has produced it.
 */
const entry = path.join(__dirname, '..', 'wasm', 'vim_engine.js');
const built = fs.existsSync(entry);

interface WasmModule {
  Session: new (text: string, line: number, col: number) => never;
}

function open(text: string): EngineSession {
  const mod = createRequire(entry)(entry) as WasmModule;
  return new EngineSession(new mod.Session(text, 0, 0));
}

/** Feed keys the way the controller does, returning the last effects. */
function feed(session: EngineSession, keys: string) {
  let last = null;
  for (const key of typedKeys(keys)) last = session.key(key);
  return last;
}

describe.skipIf(!built)('engine session', () => {
  it('runs a regex search and reports the cursor and match rank', () => {
    const session = open('alpha 42\nbeta');
    const fx = feed(session, '/\\d\\+');
    expect(fx?.pending).toBe('/\\d\\+');
    const done = session.key('<cr>');
    expect(done?.selections[0].active).toEqual({ line: 0, col: 6 });
    expect(done?.message).toBe('match 1 of 2'); // "42" and its inner "2"
    expect(done?.search).toEqual({ kind: 'committed' });
    session.dispose();
  });

  it('previews matches while the pattern is typed', () => {
    const session = open('foo bar\nbaz bar\nqux');
    expect(session.key('/')?.search).toEqual({ kind: 'active', matches: [] });
    const fx = feed(session, 'bar');
    // The peeked match is separate from the highlight pile, and the
    // cursor has not moved.
    expect(fx?.search).toEqual({
      kind: 'active',
      matches: [1, 4, 7],
      current: [0, 4, 7],
    });
    expect(fx?.selections[0]?.active).toEqual({ line: 0, col: 0 });
    const gone = session.key('<esc>');
    expect(gone?.search).toEqual({ kind: 'cancelled' });
    session.dispose();
  });

  it('substitutes across a range and reports the count', () => {
    const session = open('foo foo\nbar\nfoo');
    feed(session, ':%s/foo/baz/g');
    const fx = session.key('<cr>');
    expect(fx?.edits).toEqual([
      { start: { line: 0, col: 0 }, end: { line: 0, col: 7 }, text: 'baz baz' },
      { start: { line: 2, col: 0 }, end: { line: 2, col: 3 }, text: 'baz' },
    ]);
    expect(fx?.message).toBe('3 substitutions on 2 lines');
    expect(session.text()).toBe('baz baz\nbar\nbaz');
    expect(session.mode()).toBe('normal');
    session.dispose();
  });

  it('carries non-ASCII edit text across the binary boundary', () => {
    // Astral and multi-byte chars: the edit block's text lengths are UTF-16
    // units, so slicing the decoded blob must land on these boundaries.
    const session = open('a☃b\na');
    feed(session, ':%s/a/😀x/g');
    const fx = session.key('<cr>');
    expect(fx?.edits).toEqual([
      { start: { line: 0, col: 0 }, end: { line: 0, col: 1 }, text: '😀x' },
      { start: { line: 1, col: 0 }, end: { line: 1, col: 1 }, text: '😀x' },
    ]);
    expect(session.text()).toBe('😀x☃b\n😀x');
    session.dispose();
  });

  it('reports ex errors instead of editing', () => {
    const session = open('abc');
    feed(session, ':s/a/b/c');
    const fx = session.key('<cr>');
    expect(fx?.edits).toEqual([]);
    expect(fx?.message).toBe('the c (confirm) flag is not supported');
    expect(session.text()).toBe('abc');
    session.dispose();
  });
});

/**
 * What a `:%s` worth of edits costs to cross the boundary, prints — it
 * asserts only sanity, so run it deliberately: `PERF=1 pnpm test`.
 * The tests/perf.rs corpus shape: 20k lines, an edit on every one.
 */
describe.skipIf(!built || !process.env.PERF)('boundary perf', () => {
  it('hands a whole-buffer substitution to the host', () => {
    const text = Array.from(
      { length: 20_000 },
      (_, i) => `let value_${i} = compute(alpha, beta, gamma);`,
    ).join('\n');
    const runs: number[] = [];
    for (let run = 0; run < 3; run++) {
      const session = open(text);
      feed(session, ':%s/value_\\d\\+/VALUE/g');
      const t0 = performance.now();
      const fx = session.key('<cr>');
      runs.push(performance.now() - t0);
      expect(fx?.edits).toHaveLength(20_000);
      expect(fx?.edits[19_999].text).toContain('VALUE');
      session.dispose();
    }
    process.stdout.write(
      `:%s across the wasm boundary (cold→warm): ${runs.map((r) => r.toFixed(2)).join(', ')} ms\n`,
    );
  });
});
