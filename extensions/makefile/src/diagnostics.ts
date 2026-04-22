import * as vscode from 'vscode';
import type { AnalyzerBridge } from './analyzer';
import type { AnalyzerDiagnostic } from './types';

const SEVERITY_MAP: Record<AnalyzerDiagnostic['severity'], vscode.DiagnosticSeverity> = {
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
  if (doc.languageId !== 'makefile') return;
  const enabled = vscode.workspace
    .getConfiguration('makefile')
    .get<boolean>('diagnostics.enabled', true);
  if (!enabled) {
    collection.delete(doc.uri);
    return;
  }
  const uri = doc.uri.toString();
  bridge.updateFile(uri, doc.getText());
  const items = bridge.diagnostics(uri);
  const diags = items.map(
    (d) =>
      new vscode.Diagnostic(
        new vscode.Range(
          new vscode.Position(d.start.line, d.start.col),
          new vscode.Position(d.end.line, d.end.col),
        ),
        `${d.code}: ${d.message}`,
        SEVERITY_MAP[d.severity],
      ),
  );
  collection.set(doc.uri, diags);
}
