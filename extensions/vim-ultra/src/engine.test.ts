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
  it('runs a regex search and reports the cursor', () => {
    const session = open('alpha 42\nbeta');
    const fx = feed(session, '/\\d\\+');
    expect(fx?.pending).toBe('/\\d\\+');
    const done = session.key('<cr>');
    expect(done?.selections[0].active).toEqual({ line: 0, col: 6 });
    expect(done?.message).toBeUndefined();
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
