import type * as vscode from 'vscode';

/**
 * Cheap content-level gate: only do real work for Rust files that
 * actually contain a `diesel::table!` (or its plain `table!`) call.
 * Avoids slowing down the editor for unrelated Rust files in projects
 * that happen to also use diesel.
 */
export function isDieselSchema(doc: vscode.TextDocument): boolean {
  if (doc.languageId !== 'rust') return false;
  const text = doc.getText();
  return text.includes('diesel::table!') || /(^|\W)table!\s*[({]/.test(text);
}
