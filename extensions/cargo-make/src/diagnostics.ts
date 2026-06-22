import * as vscode from 'vscode';
import type { AnalyzerBridge } from './analyzer';
import type { AnalyzerDiagnostic } from './types';

const SEVERITY_MAP: Record<AnalyzerDiagnostic['severity'], vscode.DiagnosticSeverity> = {
  error: vscode.DiagnosticSeverity.Error,
  warning: vscode.DiagnosticSeverity.Warning,
  info: vscode.DiagnosticSeverity.Information,
  hint: vscode.DiagnosticSeverity.Hint,
};

export function isCargoMakeDocument(languageId: string): boolean {
  return languageId === 'cargo-make';
}

export function refreshDiagnostics(
  bridge: AnalyzerBridge,
  doc: vscode.TextDocument,
  collection: vscode.DiagnosticCollection,
): void {
  if (!isCargoMakeDocument(doc.languageId)) return;
  const enabled = vscode.workspace
    .getConfiguration('cargoMake')
    .get<boolean>('diagnostics.enabled', true);
  if (!enabled) {
    collection.delete(doc.uri);
    return;
  }
  const uri = doc.uri.toString();
  bridge.updateFile(uri, doc.getText());
  const items = bridge.diagnostics(uri);
  const diags = items.map((d) => {
    const diag = new vscode.Diagnostic(
      new vscode.Range(
        new vscode.Position(d.start.line, d.start.col),
        new vscode.Position(d.end.line, d.end.col),
      ),
      d.message,
      SEVERITY_MAP[d.severity],
    );
    diag.code = d.code;
    diag.source = 'cargo-make';
    return diag;
  });
  collection.set(doc.uri, diags);
}
