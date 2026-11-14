import * as vscode from 'vscode';
import type { AnalyzerBridge } from './analyzer';
import { readDiagnosticsEnabled } from './config';
import type { AnalyzerDiagnostic } from './types';

export const LANGUAGE_ID = 'ron';

const SEVERITY: Record<AnalyzerDiagnostic['severity'], vscode.DiagnosticSeverity> = {
  error: vscode.DiagnosticSeverity.Error,
  warning: vscode.DiagnosticSeverity.Warning,
  info: vscode.DiagnosticSeverity.Information,
  hint: vscode.DiagnosticSeverity.Hint,
};

export function refreshDiagnostics(
  bridge: AnalyzerBridge,
  doc: vscode.TextDocument,
  collection: vscode.DiagnosticCollection,
): void {
  if (doc.languageId !== LANGUAGE_ID) return;
  if (!readDiagnosticsEnabled(doc.uri)) {
    collection.delete(doc.uri);
    return;
  }
  const uri = doc.uri.toString();
  bridge.updateFile(uri, doc.getText());
  const diags = bridge.diagnostics(uri).map((d) => {
    const diag = new vscode.Diagnostic(
      new vscode.Range(d.start.line, d.start.col, d.end.line, d.end.col),
      d.message,
      SEVERITY[d.severity],
    );
    diag.code = d.code;
    diag.source = 'ron';
    return diag;
  });
  collection.set(doc.uri, diags);
}
