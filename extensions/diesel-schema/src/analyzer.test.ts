import * as fs from 'fs';
import * as path from 'path';
import { describe, expect, it } from 'vitest';
import { AnalyzerBridge } from './analyzer';

// The WASM bundle lives in `extensions/diesel-schema/wasm/` after `pnpm run build:wasm`.
const extensionPath = path.resolve(__dirname, '..');
const hasWasm = fs.existsSync(path.join(extensionPath, 'wasm', 'diesel_schema_analyzer.js'));

const URI = 'file:///schema.rs';
const SCHEMA = `diesel::table! {
    users (id) {
        id -> Int4,
        name -> Text,
    }
}

diesel::table! {
    posts (id) {
        id -> Int4,
        user_id -> Int4,
    }
}

diesel::joinable!(posts -> users (user_id));
diesel::allow_tables_to_appear_in_same_query!(users, posts,);
`;

/** A fresh bridge with `SCHEMA` already loaded under `URI`. */
function loaded(source = SCHEMA): AnalyzerBridge {
  const bridge = AnalyzerBridge.load(extensionPath);
  bridge.updateFile(URI, source);
  return bridge;
}

describe('AnalyzerBridge without a WASM bundle', () => {
  const bridge = AnalyzerBridge.load(path.join(extensionPath, 'does-not-exist'));

  it('reports itself as not ready', () => {
    expect(bridge.ready).toBe(false);
  });

  it('degrades to empty results instead of throwing', () => {
    expect(() => bridge.updateFile(URI, SCHEMA)).not.toThrow();
    expect(() => bridge.removeFile(URI)).not.toThrow();
    expect(bridge.diagnostics(URI)).toEqual([]);
    expect(bridge.documentSymbols(URI)).toEqual([]);
    expect(bridge.foldingRanges(URI)).toEqual([]);
    expect(bridge.complete(URI, 0, 0)).toEqual([]);
    expect(bridge.hover(URI, 0, 0)).toBeNull();
  });
});

describe.skipIf(!hasWasm)('AnalyzerBridge with the WASM analyzer', () => {
  it('loads', () => {
    expect(AnalyzerBridge.load(extensionPath).ready).toBe(true);
  });

  it('reports no diagnostics for a consistent schema', () => {
    expect(loaded().diagnostics(URI)).toEqual([]);
  });

  it('flags unknown tables in joinable! and allow-group macros', () => {
    const bridge = loaded(`${SCHEMA}
diesel::joinable!(posts -> ghosts (user_id));
diesel::allow_tables_to_appear_in_same_query!(users, missing,);
`);
    const byCode = new Map(bridge.diagnostics(URI).map((d) => [d.code, d]));

    const unknownJoin = byCode.get('DS005');
    expect(unknownJoin?.severity).toBe('warning');
    expect(unknownJoin?.message).toContain('ghosts');
    // The range covers just the offending table name on the `joinable!` line.
    expect(unknownJoin?.start.line).toBe(17);
    expect(unknownJoin?.end.col).toBe(unknownJoin!.start.col + 'ghosts'.length);

    expect(byCode.get('DS007')?.message).toContain('missing');
  });

  it('outlines tables, joinables and allow-groups', () => {
    const symbols = loaded().documentSymbols(URI);
    expect(symbols.map((s) => [s.name, s.kind, s.detail])).toEqual([
      ['users', 'Table', 'pk: id'],
      ['posts', 'Table', 'pk: id'],
      ['posts → users', 'Joinable', 'via `user_id`'],
      ['allow_tables_to_appear_in_same_query #1', 'AllowGroup', '2 tables'],
    ]);
  });

  it('nests columns under their table in the outline', () => {
    const users = loaded().documentSymbols(URI)[0];
    expect(users.children.map((c) => [c.name, c.detail, c.kind])).toEqual([
      ['id', 'Int4', 'Column'],
      ['name', 'Text', 'Column'],
    ]);
    // The full range spans the macro; the selection range is just the table name.
    expect(users.range_start.line).toBe(0);
    expect(users.range_end.line).toBe(5);
    expect(users.selection_start).toEqual({ line: 1, col: 4 });
  });

  it('produces one folding range per table macro', () => {
    const ranges = loaded().foldingRanges(URI);
    expect(ranges).toEqual([
      { start_line: 0, end_line: 5, kind: 'Region' },
      { start_line: 7, end_line: 12, kind: 'Region' },
    ]);
  });

  it('completes table names inside joinable!', () => {
    const bridge = loaded(`${SCHEMA}diesel::joinable!(po -> users (user_id));\n`);
    // Line 16 is the appended macro; column 20 sits right after the `po` prefix.
    const items = bridge.complete(URI, 16, 20);
    expect(items).toEqual([
      {
        label: 'posts',
        detail: 'diesel table',
        kind: 'Table',
        insert_text: 'posts',
        replace_length: 2,
      },
    ]);
  });

  it('completes FK columns of the child table', () => {
    const bridge = loaded(`${SCHEMA}diesel::joinable!(posts -> users (us));\n`);
    const items = bridge.complete(URI, 16, 36);
    expect(items.map((i) => [i.label, i.detail, i.kind, i.replace_length])).toEqual([
      ['user_id', 'Int4', 'Column', 2],
    ]);
    // `replace_length` tracks how much of the prefix the edit overwrites.
    expect(bridge.complete(URI, 16, 35)[0].replace_length).toBe(1);
  });

  it('hovers a table with its full column list', () => {
    const hover = loaded().hover(URI, 1, 5);
    expect(hover?.contents).toContain('**table `users`**');
    expect(hover?.contents).toContain('primary key: `id`');
    expect(hover?.contents).toContain('| `name` | `Text` |');
    expect(hover?.start).toEqual({ line: 1, col: 4 });
  });

  it('hovers a column with its SQL type', () => {
    const hover = loaded().hover(URI, 10, 10);
    expect(hover?.contents).toBe('**posts.user_id**: `Int4`');
    expect(hover?.start).toEqual({ line: 10, col: 8 });
    expect(hover?.end).toEqual({ line: 10, col: 15 });
  });

  it('returns nothing for positions with no symbol', () => {
    expect(loaded().hover(URI, 6, 0)).toBeNull();
  });

  it('forgets a file after removeFile', () => {
    const bridge = loaded();
    expect(bridge.documentSymbols(URI)).not.toEqual([]);
    bridge.removeFile(URI);
    expect(bridge.documentSymbols(URI)).toEqual([]);
    expect(bridge.diagnostics(URI)).toEqual([]);
    expect(bridge.hover(URI, 1, 5)).toBeNull();
  });
});
