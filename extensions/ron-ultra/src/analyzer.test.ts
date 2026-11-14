import * as fs from 'fs';
import * as path from 'path';
import { describe, expect, it } from 'vitest';
import { AnalyzerBridge } from './analyzer';
import type { FormatOptions } from './types';

// The WASM bundle lives in `extensions/ron-ultra/wasm/` after `pnpm run build:wasm`.
const extensionPath = path.resolve(__dirname, '..');
const hasWasm = fs.existsSync(path.join(extensionPath, 'wasm', 'ron_analyzer.js'));

const URI = 'file:///config.ron';
const OPTIONS: FormatOptions = {
  tab_size: 4,
  insert_spaces: true,
  print_width: 80,
  insert_final_newline: true,
};

function loaded(source: string): AnalyzerBridge {
  const bridge = AnalyzerBridge.load(extensionPath);
  bridge.updateFile(URI, source);
  return bridge;
}

describe('AnalyzerBridge without a WASM bundle', () => {
  const bridge = AnalyzerBridge.load(path.join(extensionPath, 'does-not-exist'));

  it('degrades to empty answers instead of throwing', () => {
    expect(bridge.ready).toBe(false);
    bridge.updateFile(URI, '(a: 1)');
    expect(bridge.diagnostics(URI)).toEqual([]);
    expect(bridge.documentSymbols(URI)).toEqual([]);
    expect(bridge.hover(URI, 0, 1)).toBeNull();
    expect(bridge.selectionRanges(URI, [{ line: 0, col: 1 }])).toEqual([]);
    expect(bridge.formatting(URI, OPTIONS)).toBeNull();
  });
});

describe.skipIf(!hasWasm)('the WASM analyzer', () => {
  it('reports diagnostics with codes and positions', () => {
    const diags = loaded('(\n    a: 1,\n    a: 2,\n    b: 0b12,\n)').diagnostics(URI);
    expect(diags.map((d) => [d.code, d.severity, d.start.line])).toEqual([
      ['RON009', 'error', 2],
      ['RON004', 'error', 3],
    ]);
  });

  it('formats, preserving comments', () => {
    const edit = loaded('Config( // root\nsize:(800,600),\n  name:"x", // why\n)').formatting(
      URI,
      OPTIONS,
    );
    expect(edit?.start).toEqual({ line: 0, col: 0 });
    expect(edit?.new_text).toBe(
      'Config( // root\n    size: (800, 600),\n    name: "x", // why\n)\n',
    );
  });

  it('declines to format a broken file', () => {
    expect(loaded('(a: 1').formatting(URI, OPTIONS)).toBeNull();
  });

  it('builds a nested outline', () => {
    const [root] = loaded('Config(window: (w: 1), keys: {"up": Up})').documentSymbols(URI);
    expect(root.name).toBe('Config');
    expect(root.kind).toBe('Struct');
    expect(root.children.map((c) => [c.name, c.kind])).toEqual([
      ['window', 'Struct'],
      ['keys', 'Object'],
    ]);
  });

  it('hovers with the value path', () => {
    const hover = loaded('(window: (size: (800, 600)))').hover(URI, 0, 23);
    expect(hover?.contents).toBe('`window.size[1]` · integer');
  });

  it('expands the selection outward', () => {
    const [chain] = loaded('(a: [1])').selectionRanges(URI, [{ line: 0, col: 5 }]);
    expect(chain.map((r) => [r.start.col, r.end.col])).toEqual([
      [5, 6],
      [4, 7],
      [1, 7],
      [0, 8],
    ]);
  });

  it('finds the bundled example clean and already formatted', () => {
    const source = fs.readFileSync(path.join(extensionPath, 'examples', 'game-config.ron'), 'utf8');
    const bridge = loaded(source);
    expect(bridge.diagnostics(URI)).toEqual([]);
    expect(bridge.formatting(URI, OPTIONS)).toBeNull();
  });

  it('folds multi-line containers', () => {
    expect(loaded('[\n    1,\n    2,\n]').foldingRanges(URI)).toEqual([
      { start_line: 0, end_line: 2, kind: 'Region' },
    ]);
  });
});
