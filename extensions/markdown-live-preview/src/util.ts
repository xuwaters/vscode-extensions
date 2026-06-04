import * as crypto from 'crypto';
import * as vscode from 'vscode';

/** Generate a random nonce string for webview CSP. */
export function getNonce(): string {
  return crypto.randomBytes(16).toString('base64');
}

/** File extensions this extension treats as markdown. */
const MARKDOWN_EXTENSIONS = /\.(md|markdown|mdx)$/i;

/** Whether a document should be previewable as markdown. */
export function isMarkdownDocument(document: vscode.TextDocument): boolean {
  return (
    document.languageId === 'markdown' ||
    document.languageId === 'mdx' ||
    MARKDOWN_EXTENSIONS.test(document.uri.fsPath)
  );
}
