// End-to-end checks through the real WASM artifact. The suite skips
// itself when the bundle has not been built (`pnpm run build:wasm`).

import * as fs from 'fs';
import * as path from 'path';
import { describe, expect, it } from 'vitest';
import { AnalyzerBridge } from './analyzer.js';

const extensionPath = path.resolve(__dirname, '..');
const hasWasm = fs.existsSync(path.join(extensionPath, 'wasm', 'json_analyzer.js'));

describe.skipIf(!hasWasm)('the WASM analyzer', () => {
  const bridge = AnalyzerBridge.load(extensionPath);
  const options = {
    tab_size: 2,
    insert_spaces: true,
    sort_keys: false,
    insert_final_newline: true,
  };

  it('reports flavor-aware diagnostics', () => {
    bridge.updateFile('t://a.json', '// nope\n{"a": 1,}', 'json');
    const codes = bridge.diagnostics('t://a.json').map((d) => d.code);
    expect(codes).toContain('JSON008');
    expect(codes).toContain('JSON009');

    bridge.updateFile('t://a.jsonc', '// fine\n{"a": 1,}', 'jsonc');
    expect(bridge.diagnostics('t://a.jsonc')).toEqual([]);
  });

  it('formats and applies cleanly', () => {
    bridge.updateFile('t://b.json', '{"b":2,"a":1}', 'json');
    const edit = bridge.formatting('t://b.json', options);
    expect(edit?.new_text).toBe('{\n  "b": 2,\n  "a": 1\n}\n');
  });

  it('sorts keys recursively', () => {
    bridge.updateFile('t://c.json', '{"b":{"z":1,"a":2},"a":3}', 'json');
    const edit = bridge.sortKeys('t://c.json', options);
    expect(edit?.new_text).toBe('{\n  "a": 3,\n  "b": {\n    "a": 2,\n    "z": 1\n  }\n}\n');
  });

  it('builds a jsonl table', () => {
    bridge.updateFile('t://d.jsonl', '{"a": 1, "b": "x"}\n{"a": 2}\n', 'jsonl');
    const table = bridge.jsonlTable('t://d.jsonl', 100);
    expect(table?.columns).toEqual(['a', 'b']);
    expect(table?.rows.map((r) => r.cells)).toEqual([
      ['1', 'x'],
      ['2', ''],
    ]);
  });

  it('hovers with a JSON path', () => {
    bridge.updateFile('t://e.json', '{"outer": {"inner": 5}}', 'json');
    const hover = bridge.hover('t://e.json', 0, 20);
    expect(hover?.contents).toContain('$.outer.inner');
  });
});

describe('without a WASM bundle', () => {
  const bridge = AnalyzerBridge.empty();

  it('degrades to empty results', () => {
    bridge.updateFile('t://x', '{}', 'json');
    expect(bridge.diagnostics('t://x')).toEqual([]);
    expect(bridge.documentSymbols('t://x')).toEqual([]);
    expect(bridge.foldingRanges('t://x')).toEqual([]);
    expect(bridge.hover('t://x', 0, 0)).toBeNull();
    expect(
      bridge.formatting('t://x', {
        tab_size: 2,
        insert_spaces: true,
        sort_keys: false,
        insert_final_newline: true,
      }),
    ).toBeNull();
    expect(bridge.jsonlTable('t://x', 10)).toBeNull();
  });
});
