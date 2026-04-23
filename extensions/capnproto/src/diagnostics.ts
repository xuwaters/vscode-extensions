import * as vscode from 'vscode';
import type { AnalyzerBridge } from './analyzer';

export const CAPNP_LANGUAGE_ID = 'capnp';

export function isCapnpDocument(languageId: string): boolean {
  return languageId === CAPNP_LANGUAGE_ID;
}

export function refreshDiagnostics(
  bridge: AnalyzerBridge,
  doc: vscode.TextDocument,
  collection: vscode.DiagnosticCollection,
): void {
  const enabled = vscode.workspace
    .getConfiguration('capnp')
    .get<boolean>('diagnostics.enabled', true);

  const uri = doc.uri.toString();
  bridge.updateFile(uri, doc.getText());

  if (!enabled) {
    collection.set(doc.uri, []);
    return;
  }

  const items = bridge.diagnostics(uri).map((d) => {
    const severity =
      d.severity === 'warning'
        ? vscode.DiagnosticSeverity.Warning
        : vscode.DiagnosticSeverity.Error;
    const range = new vscode.Range(
      new vscode.Position(d.start.line, d.start.col),
      new vscode.Position(d.end.line, d.end.col),
    );
    const diag = new vscode.Diagnostic(range, d.message, severity);
    diag.code = d.code;
    diag.source = 'capnp';
    return diag;
  });

  collection.set(doc.uri, items);
}
