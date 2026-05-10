import * as path from 'path';
import * as fs from 'fs';
import type { WasmModule } from './types.js';

/**
 * Load the wasm-pack-built `log_parser` module from `dist/../wasm/log_parser.js`.
 * Returns null and logs to console if the bundle is missing — typical when the
 * extension is run before `pnpm run build:wasm` finishes.
 */
export function loadWasm(extensionPath: string): WasmModule | null {
  const entry = path.join(extensionPath, 'wasm', 'log_parser.js');
  if (!fs.existsSync(entry)) {
    console.warn(
      'log-parser WASM bundle not found. Run `pnpm run build:wasm` in extensions/log-viewer.',
    );
    return null;
  }
  try {
    // eslint-disable-next-line @typescript-eslint/no-require-imports
    const mod = require(entry) as WasmModule;
    mod.init();
    return mod;
  } catch (e) {
    console.error('Failed to load log-parser WASM:', e);
    return null;
  }
}
