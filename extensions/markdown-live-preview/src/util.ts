import * as crypto from 'crypto';

/** Generate a random nonce string for webview CSP. */
export function getNonce(): string {
  return crypto.randomBytes(16).toString('base64');
}
