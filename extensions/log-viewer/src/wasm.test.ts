import * as path from 'path';
import { describe, expect, it } from 'vitest';
import { loadWasm } from './wasm.js';
import type { ParsedLines } from './types.js';

const ESC = '\x1b';

describe('log-parser WASM', () => {
  // The WASM bundle lives in `extensions/log-viewer/wasm/` after `pnpm run build:wasm`.
  const extensionPath = path.resolve(__dirname, '..');
  const wasm = loadWasm(extensionPath);

  it.skipIf(!wasm)('renders ANSI through allLinesJson', () => {
    const idx = new wasm!.LogIndex(`${ESC}[31merror\nokay${ESC}[0m`);
    try {
      const parsed = JSON.parse(idx.allLinesJson()) as ParsedLines;
      expect(parsed.text).toEqual(['error', 'okay']);
      expect(parsed.html[0]).toContain('color:var(--vscode-terminal-ansiRed)');
    } finally {
      idx.free();
    }
  });

  it.skipIf(!wasm)('matches filter rules and returns Uint8Array', () => {
    const idx = new wasm!.LogIndex('first ERROR line\nsecond INFO line\nthird');
    try {
      const rules = [
        { name: 'errors', pattern: 'ERROR', regex: false, enabled: true },
        { name: 'info', pattern: 'INFO', regex: false, enabled: true },
      ];
      const out = idx.matchFilters(JSON.stringify(rules));
      expect(Array.from(out)).toEqual([1, 2, 0]);
    } finally {
      idx.free();
    }
  });

  it.skipIf(!wasm)('search returns matching line indices', () => {
    const idx = new wasm!.LogIndex('alpha\nbeta\nALPHA');
    try {
      const ci = idx.search('alpha', false, false);
      expect(Array.from(ci)).toEqual([0, 2]);
      const cs = idx.search('alpha', false, true);
      expect(Array.from(cs)).toEqual([0]);
    } finally {
      idx.free();
    }
  });
});
