// Page shell for the JSON Lines table preview: strict CSP, a nonce'd
// module script, and one mount point the bundle takes over.

import * as vscode from 'vscode';

function makeNonce(): string {
  const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789';
  let nonce = '';
  for (let i = 0; i < 32; i += 1) {
    nonce += alphabet.charAt(Math.floor(Math.random() * alphabet.length));
  }
  return nonce;
}

export function previewHtml(webview: vscode.Webview, extensionUri: vscode.Uri): string {
  const nonce = makeNonce();
  const script = webview.asWebviewUri(vscode.Uri.joinPath(extensionUri, 'dist', 'webview.js'));
  const csp = [
    `default-src 'none'`,
    `img-src ${webview.cspSource}`,
    `style-src ${webview.cspSource} 'unsafe-inline'`,
    `script-src 'nonce-${nonce}'`,
    `font-src ${webview.cspSource}`,
  ].join('; ');

  return `<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta http-equiv="Content-Security-Policy" content="${csp}" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>JSON Lines Table</title>
    <style nonce="${nonce}">
      html, body { height: 100%; margin: 0; padding: 0; overflow: hidden; }
      #app { height: 100%; }
    </style>
  </head>
  <body>
    <div id="app"></div>
    <script nonce="${nonce}" type="module" src="${script}"></script>
  </body>
</html>`;
}
