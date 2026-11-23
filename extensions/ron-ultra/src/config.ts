// The `ronUltra.*` settings, gathered into the one object the server
// deserializes. The shape must match `ron_lsp_core::Settings`; it is spelled
// out so a typo is a TypeScript error here instead of a silently-defaulted
// setting there.

import * as vscode from 'vscode';

export const CONFIG_SECTION = 'ronUltra';

export interface Settings {
  diagnostics: { enabled: boolean };
  format: { enabled: boolean; printWidth: number };
  schema: { enabled: boolean };
}

export function read(): Settings {
  const c = vscode.workspace.getConfiguration(CONFIG_SECTION);
  return {
    diagnostics: { enabled: c.get<boolean>('diagnostics.enabled', true) },
    format: {
      enabled: c.get<boolean>('format.enabled', true),
      printWidth: Math.max(0, Math.floor(c.get<number>('format.printWidth', 80))),
    },
    schema: { enabled: c.get<boolean>('schema.enabled', true) },
  };
}
