// Diagnostics for JSON5 and JSON Lines only. JSON and JSONC are already
// validated by VSCode's built-in JSON language service; doubling its
// squiggles would help nobody.

import * as vscode from 'vscode';
import type { AnalyzerBridge } from './analyzer.js';
import { readDiagnosticsEnabled } from './config.js';
import type { AnalyzerDiagnostic } from './types.js';

export const DIAGNOSTIC_LANGUAGES = new Set(['json5', 'jsonl']);

const SEVERITY_MAP: Record<AnalyzerDiagnostic['severity'], vscode.DiagnosticSeverity> = {
  error: vscode.DiagnosticSeverity.Error,
  warning: vscode.DiagnosticSeverity.Warning,
  info: vscode.DiagnosticSeverity.Information,
  hint: vscode.DiagnosticSeverity.Hint,
};

export function refreshDiagnostics(
  collection: vscode.DiagnosticCollection,
  bridge: AnalyzerBridge,
  document: vscode.TextDocument,
): void {
  if (!DIAGNOSTIC_LANGUAGES.has(document.languageId)) return;
  if (!readDiagnosticsEnabled(document.uri)) {
    collection.delete(document.uri);
    return;
  }
  const uri = document.uri.toString();
  bridge.updateFile(uri, document.getText(), document.languageId);
  const diagnostics = bridge.diagnostics(uri).map((d) => {
    const diagnostic = new vscode.Diagnostic(
      new vscode.Range(
        new vscode.Position(d.start.line, d.start.col),
        new vscode.Position(d.end.line, d.end.col),
      ),
      d.message,
      SEVERITY_MAP[d.severity],
    );
    diagnostic.code = d.code;
    diagnostic.source = 'json-ultra';
    return diagnostic;
  });
  collection.set(document.uri, diagnostics);
}
