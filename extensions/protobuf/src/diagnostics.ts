import * as vscode from 'vscode';
import type { AnalyzerBridge } from './analyzer';
import type { AnalyzerDiagnostic } from './types';

const SEVERITY_MAP: Record<AnalyzerDiagnostic['severity'], vscode.DiagnosticSeverity> = {
  error: vscode.DiagnosticSeverity.Error,
  warning: vscode.DiagnosticSeverity.Warning,
  info: vscode.DiagnosticSeverity.Information,
  hint: vscode.DiagnosticSeverity.Hint,
};

export type AnalyzerLanguage = 'proto3' | 'textproto';

export function isAnalyzerLanguage(languageId: string): languageId is AnalyzerLanguage {
  return languageId === 'proto3' || languageId === 'textproto';
}

export function refreshDiagnostics(
  bridge: AnalyzerBridge,
  doc: vscode.TextDocument,
  collection: vscode.DiagnosticCollection,
): void {
  const uri = doc.uri.toString();
  let items: AnalyzerDiagnostic[];
  if (doc.languageId === 'proto3') {
    bridge.updateFile(uri, doc.getText());
    items = bridge.diagnostics(uri);
  } else if (doc.languageId === 'textproto') {
    bridge.updateTextprotoFile(uri, doc.getText());
    items = bridge.textprotoDiagnostics(uri);
  } else {
    return;
  }
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
