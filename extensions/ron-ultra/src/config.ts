// Typed readers over the `ronUltra.*` settings, per-resource scoped.

import * as vscode from 'vscode';
import type { FormatOptions } from './types';

export const CONFIG_SECTION = 'ronUltra';

function config(resource: vscode.Uri): vscode.WorkspaceConfiguration {
  return vscode.workspace.getConfiguration(CONFIG_SECTION, resource);
}

export function readDiagnosticsEnabled(resource: vscode.Uri): boolean {
  return config(resource).get<boolean>('diagnostics.enabled', true);
}

/** `null` when formatting is switched off. */
export function readFormatOptions(
  document: vscode.TextDocument,
  options: vscode.FormattingOptions,
): FormatOptions | null {
  const ron = config(document.uri);
  if (!ron.get<boolean>('format.enabled', true)) return null;
  const files = vscode.workspace.getConfiguration('files', document.uri);
  return {
    tab_size: Math.max(1, Math.floor(options.tabSize)),
    insert_spaces: options.insertSpaces,
    print_width: Math.max(0, Math.floor(ron.get<number>('format.printWidth', 80))),
    insert_final_newline: files.get<boolean>('insertFinalNewline', true),
  };
}
