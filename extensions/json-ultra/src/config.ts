// Typed readers over the `jsonUltra.*` settings, per-resource scoped.

import * as vscode from 'vscode';
import type { FormatOptions } from './types.js';

export const CONFIG_SECTION = 'jsonUltra';

function config(resource: vscode.Uri | undefined): vscode.WorkspaceConfiguration {
  return vscode.workspace.getConfiguration(CONFIG_SECTION, resource);
}

export function readFormatSortKeys(resource: vscode.Uri | undefined): boolean {
  return config(resource).get<boolean>('format.sortKeys', false);
}

export type OxcMode = 'auto' | 'never';

export function readOxcMode(resource: vscode.Uri | undefined): OxcMode {
  const value = config(resource).get<string>('format.oxc', 'auto');
  return value === 'never' ? 'never' : 'auto';
}

export function readOxcPath(resource: vscode.Uri | undefined): string {
  return config(resource).get<string>('oxc.path', '').trim();
}

export function readDiagnosticsEnabled(resource: vscode.Uri | undefined): boolean {
  return config(resource).get<boolean>('diagnostics.enabled', true);
}

export function readPreviewMaxRows(resource: vscode.Uri | undefined): number {
  const value = config(resource).get<number>('preview.maxRows', 100_000);
  return Math.max(1, Math.floor(value));
}

export function readPreviewMaxFileSize(resource: vscode.Uri | undefined): number {
  const value = config(resource).get<number>('preview.maxFileSizeBytes', 32 * 1024 * 1024);
  return Math.max(1024, Math.floor(value));
}

/** Marshal editor state + settings into the Rust `FormatOptions`. */
export function formatOptionsFor(
  document: vscode.TextDocument,
  options: Pick<vscode.FormattingOptions, 'tabSize' | 'insertSpaces'>,
  sortKeys: boolean,
): FormatOptions {
  const files = vscode.workspace.getConfiguration('files', document.uri);
  return {
    tab_size: Math.max(1, Math.floor(options.tabSize)),
    insert_spaces: options.insertSpaces,
    sort_keys: sortKeys,
    insert_final_newline: files.get<boolean>('insertFinalNewline', true),
  };
}
