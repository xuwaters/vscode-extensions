import * as fs from 'fs';
import * as path from 'path';
import { describe, expect, it } from 'vitest';
import { AnalyzerBridge } from './analyzer';
import type { AnalyzerTextEdit, FormatOptions, LineCol } from './types';

// The WASM bundle lives in `extensions/dotenv/wasm/` after `pnpm run build:wasm`.
const extensionPath = path.resolve(__dirname, '..');
const hasWasm = fs.existsSync(path.join(extensionPath, 'wasm', 'dotenv_analyzer.js'));

const URI = 'file:///.env';
const DEFAULTS: FormatOptions = { max_blank_lines: 1, insert_final_newline: true };

function loaded(source: string): AnalyzerBridge {
  const bridge = AnalyzerBridge.load(extensionPath);
  bridge.updateFile(URI, source);
  return bridge;
}

function offsetOf(source: string, pos: LineCol): number {
  const lines = source.split('\n');
  let offset = 0;
  for (let i = 0; i < pos.line; i++) offset += lines[i].length + 1;
  return offset + pos.col;
}

/** What the editor ends up with — a null edit leaves the source alone. */
function apply(source: string, edit: AnalyzerTextEdit | null): string {
  if (!edit) return source;
  return (
    source.slice(0, offsetOf(source, edit.start)) +
    edit.new_text +
    source.slice(offsetOf(source, edit.end))
  );
}

describe('AnalyzerBridge without a WASM bundle', () => {
  const bridge = AnalyzerBridge.load(path.join(extensionPath, 'does-not-exist'));

  it('degrades to no formatting edits instead of throwing', () => {
    expect(bridge.ready).toBe(false);
    expect(bridge.formatting(URI, DEFAULTS)).toBeNull();
    expect(bridge.formattingRange(URI, 0, 0, DEFAULTS)).toBeNull();
  });
});

describe.skipIf(!hasWasm)('formatting through the WASM analyzer', () => {
  it('normalizes assignments, comments and blank runs', () => {
    const source = '\n\n  export  API_URL =  http://x   # prod\n\n\n\nDEBUG=1';
    const edit = loaded(source).formatting(URI, DEFAULTS);
    expect(apply(source, edit)).toBe('export API_URL=http://x # prod\n\nDEBUG=1\n');
  });

  it('leaves quoted values byte-for-byte alone', () => {
    const source = '  MSG = "line one\n  line two"\nRAW=\'  keep  \'\n';
    const edit = loaded(source).formatting(URI, DEFAULTS);
    expect(apply(source, edit)).toBe('MSG="line one\n  line two"\nRAW=\'  keep  \'\n');
  });

  it('produces no edit for an already formatted file', () => {
    expect(loaded('A=1\n\nB=2 # note\n').formatting(URI, DEFAULTS)).toBeNull();
  });

  it('declines to format a file with an unclosed quote', () => {
    expect(loaded('A = "oops\n').formatting(URI, DEFAULTS)).toBeNull();
  });

  it('honours max_blank_lines and insert_final_newline', () => {
    const source = 'A=1\n\n\n\nB=2\n';
    const options: FormatOptions = { max_blank_lines: 2, insert_final_newline: false };
    expect(apply(source, loaded(source).formatting(URI, options))).toBe('A=1\n\n\nB=2');
  });

  it('rewrites only the selected lines on a range format', () => {
    const source = 'A = 1\nB = 2\nC = 3\n';
    const edit = loaded(source).formattingRange(URI, 1, 1, DEFAULTS);
    expect(edit?.start).toEqual({ line: 1, col: 0 });
    expect(edit?.end).toEqual({ line: 2, col: 0 });
    expect(apply(source, edit)).toBe('A = 1\nB=2\nC = 3\n');
  });
});
