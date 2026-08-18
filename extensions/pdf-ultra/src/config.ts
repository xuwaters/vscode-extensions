import * as vscode from 'vscode';
import type { FitMode, ViewerSettings } from './messages.js';

export const CONFIG_SECTION = 'pdfUltra';

/**
 * Read the configuration the webview cares about, resolved for one document —
 * settings are per-resource, so a workspace folder can differ from the window.
 */
export function readSettings(uri?: vscode.Uri): ViewerSettings {
  const config = vscode.workspace.getConfiguration(CONFIG_SECTION, uri ?? null);
  return {
    defaultZoom: asFit(config.get<string>('defaultZoom')),
    background: asEnum(config.get<string>('background'), ['editor', 'white', 'gray'], 'editor'),
    invertColors: asEnum(
      config.get<string>('invertColors'),
      ['never', 'always', 'auto'],
      'never',
    ),
    textLayer: config.get<boolean>('textLayer') ?? true,
    links: config.get<boolean>('links') ?? true,
    outlineVisible: config.get<boolean>('outline.visible') ?? false,
    outlineWidth: clamp(config.get<number>('outline.width'), 140, 720, 240),
    maxCanvasPixels: clamp(config.get<number>('maxCanvasPixels'), 1 << 20, 1 << 30, 16 << 20),
    renderAhead: clamp(config.get<number>('renderAhead'), 0, 4, 1),
  };
}

/** Whether the file is reloaded when it changes on disk. Host-side only. */
export function readReloadOnChange(uri?: vscode.Uri): boolean {
  return (
    vscode.workspace.getConfiguration(CONFIG_SECTION, uri ?? null).get<boolean>('reloadOnChange') ??
    true
  );
}

/** Whether a document reopens on the page it was last read to. Host-side only. */
export function readRememberPosition(uri?: vscode.Uri): boolean {
  return (
    vscode.workspace
      .getConfiguration(CONFIG_SECTION, uri ?? null)
      .get<boolean>('rememberPosition') ?? true
  );
}

function asFit(value: string | undefined): FitMode {
  return asEnum(value, ['fit-width', 'fit-page', 'actual'] as const, 'fit-width');
}

function asEnum<T extends string>(
  value: string | undefined,
  allowed: readonly T[],
  fallback: T,
): T {
  return allowed.includes(value as T) ? (value as T) : fallback;
}

function clamp(
  value: number | undefined,
  min: number,
  max: number,
  fallback: number,
): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) return fallback;
  return Math.min(max, Math.max(min, value));
}
