import * as vscode from 'vscode';

/**
 * The webview document.
 *
 * There is almost nothing in it: the table is `<csv-grid>`, a FAST element that
 * renders its own chrome into its own shadow root, so the only markup the host
 * owns is the empty tag and the two rules that size it — a document that fills
 * the tab, and a policy.
 *
 * The tag is written here rather than constructed by the bootstrap because
 * fast-element 3 defines a custom element *asynchronously*: a script that
 * constructs the class the moment its module has evaluated gets `Illegal
 * constructor`, because the name is not registered yet. Markup has no such
 * problem — the parser makes an ordinary unknown element and the browser
 * upgrades it in place once the definition lands.
 *
 * The CSP is narrow because it can afford to be. Nothing in this page loads
 * anything: the file's text arrives over `postMessage`, the grid is drawn from
 * it, and there is no image, no font, no fetch and no worker anywhere in the
 * viewer. So `default-src 'none'` stands, `script-src` is one nonce — no inline
 * script and no injected script can run, because no value permits one — and the
 * only concession is `'unsafe-inline'` for styles, which is what a shadow root's
 * adopted stylesheet needs. A cell holding `<script>` or `javascript:` is text
 * in a `textContent`, never markup; the policy is the second line rather than
 * the first.
 */
export function html(webview: vscode.Webview, extensionUri: vscode.Uri): string {
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
    <title>CSV Ultra</title>
    <style nonce="${nonce}">
      html,
      body {
        height: 100%;
        margin: 0;
        padding: 0;
        overflow: hidden;
      }
      csv-grid {
        display: block;
        height: 100%;
      }
    </style>
  </head>
  <body>
    <csv-grid></csv-grid>
    <script nonce="${nonce}" type="module" src="${script}"></script>
  </body>
</html>`;
}

function makeNonce(): string {
  const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789';
  let out = '';
  for (let i = 0; i < 32; i += 1) {
    out += alphabet[Math.floor(Math.random() * alphabet.length)];
  }
  return out;
}
