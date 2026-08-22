import * as vscode from 'vscode';
import type { QuoteStyle } from './csv/serialize.js';
import type { GridSettings } from './messages.js';

export const CONFIG_SECTION = 'csvUltra';

/**
 * Read the configuration the page cares about, resolved for one document —
 * settings are per-resource, so a workspace folder can differ from the window.
 */
export function readGridSettings(uri?: vscode.Uri): GridSettings {
  const config = at(uri);
  return {
    headerRow: asEnum(config.get<string>('headerRow'), ['auto', 'always', 'never'], 'auto'),
    rowHeight: clamp(config.get<number>('rowHeight'), 16, 400, 24),
    columnWidth: clamp(config.get<number>('columnWidth'), 40, 1200, 140),
    maxColumnWidth: clamp(config.get<number>('maxColumnWidth'), 60, 2000, 480),
    autoFitOnOpen: config.get<boolean>('autoFitOnOpen') ?? true,
    fontSize: clamp(config.get<number>('fontSize'), 0, 48, 0),
    fontFamily: asEnum(config.get<string>('fontFamily'), ['editor', 'ui'], 'editor'),
    wrap: config.get<boolean>('wrap') ?? false,
    zebraStripes: config.get<boolean>('zebraStripes') ?? true,
    alignNumbers: config.get<boolean>('alignNumbers') ?? true,
  };
}

/** The configured delimiter, or `auto` — resolved against the file by `resolveDialect`. */
export function readDelimiter(uri?: vscode.Uri): string {
  const value = at(uri).get<string>('delimiter');
  return typeof value === 'string' && value.length > 0 ? value : 'auto';
}

/** How a rewritten record spells its quotes. */
export function readQuoteStyle(uri?: vscode.Uri): QuoteStyle {
  return asEnum(at(uri).get<string>('quoteStyle'), ['preserve', 'minimal', 'always'], 'preserve');
}

/** Whether a file reopens with the widths, heights and sort it was left with. */
export function readRememberLayout(uri?: vscode.Uri): boolean {
  return at(uri).get<boolean>('rememberLayout') ?? true;
}

/** The largest file a table will be built from. */
export function readMaxFileSize(uri?: vscode.Uri): number {
  return clamp(at(uri).get<number>('maxFileSizeBytes'), 1 << 16, Number.MAX_SAFE_INTEGER, 32 << 20);
}

/** Whether the text editor colours its columns, and up to what size. */
export function readRainbow(uri?: vscode.Uri): { enabled: boolean; maxBytes: number } {
  const config = at(uri);
  return {
    enabled: config.get<boolean>('rainbow.enabled') ?? true,
    maxBytes: clamp(
      config.get<number>('rainbow.maxFileSizeBytes'),
      4096,
      Number.MAX_SAFE_INTEGER,
      8 << 20,
    ),
  };
}

/** Whether the cell under the cursor is announced in the status bar. */
export function readStatusBar(uri?: vscode.Uri): boolean {
  return at(uri).get<boolean>('statusBar') ?? true;
}

function at(uri?: vscode.Uri): vscode.WorkspaceConfiguration {
  return vscode.workspace.getConfiguration(CONFIG_SECTION, uri ?? null);
}

function asEnum<T extends string>(
  value: string | undefined,
  allowed: readonly T[],
  fallback: T,
): T {
  return allowed.includes(value as T) ? (value as T) : fallback;
}

function clamp(value: number | undefined, min: number, max: number, fallback: number): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) return fallback;
  return Math.min(max, Math.max(min, value));
}
