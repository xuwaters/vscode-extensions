// Host ⇄ webview protocol for the JSON Lines table preview, plus the
// validator applied to everything the page sends back. The preview is
// read-only, so the inbound surface is small — but it still runs in a
// webview, so nothing it says is trusted without a shape check.
//
// Lives in src/ (not webview/) because both bundles import it and the
// packaging step already excludes src/ from the VSIX.

import type { JsonlTable } from './types.js';

export type HostToWebview =
  | {
      readonly type: 'load';
      readonly name: string;
      readonly table: JsonlTable;
    }
  | { readonly type: 'refused'; readonly bytes: number; readonly limit: number }
  | { readonly type: 'noParser' }
  | { readonly type: 'visible' };

export type WebviewToHost =
  | { readonly type: 'ready' }
  | { readonly type: 'openLine'; readonly line: number }
  | { readonly type: 'openText' }
  | { readonly type: 'copy'; readonly text: string }
  | { readonly type: 'error'; readonly message: string };

const MAX_LINE = 100_000_000;
const MAX_COPY = 64 * 1024 * 1024;
const MAX_ERROR = 4_000;

export function parseWebviewMessage(value: unknown): WebviewToHost | null {
  if (typeof value !== 'object' || value === null) return null;
  const message = value as { type?: unknown; line?: unknown; text?: unknown; message?: unknown };
  switch (message.type) {
    case 'ready':
      return { type: 'ready' };
    case 'openText':
      return { type: 'openText' };
    case 'openLine': {
      const line = message.line;
      if (typeof line !== 'number' || !Number.isInteger(line)) return null;
      if (line < 0 || line > MAX_LINE) return null;
      return { type: 'openLine', line };
    }
    case 'copy': {
      const text = message.text;
      if (typeof text !== 'string' || text.length > MAX_COPY) return null;
      return { type: 'copy', text };
    }
    case 'error': {
      const text = message.message;
      if (typeof text !== 'string') return null;
      return { type: 'error', message: text.slice(0, MAX_ERROR) };
    }
    default:
      return null;
  }
}
