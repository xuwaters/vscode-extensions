import * as vscode from 'vscode';

/**
 * The webview document.
 *
 * The CSP is the load-bearing part. Page SVG is engine-generated — `typst_svg`
 * emits a fixed vocabulary of `<path>`, `<use>`, `<symbol>`, `<g>`, `<defs>`,
 * `<image>`, and `<text>` from the layout frame, and a typst document has no
 * escape hatch into raw markup in the paged export target. But the *content* is
 * still attacker-influenced, because a document can come from anywhere, so the
 * policy does not rely on that:
 *
 * * `default-src 'none'` — nothing loads unless named below.
 * * `script-src 'nonce-…'` — no inline script in injected SVG can run, because
 *   no `script-src` value permits one.
 * * `img-src` is limited to the webview's own origin and `data:`, which is how
 *   the compiler embeds raster images.
 * * No remote origins at all: no CDN, no web fonts, no telemetry.
 */
export function html(webview: vscode.Webview, extensionUri: vscode.Uri): string {
  const nonce = makeNonce();
  const script = webview.asWebviewUri(
    vscode.Uri.joinPath(extensionUri, 'dist', 'webview.js'),
  );

  const csp = [
    `default-src 'none'`,
    `img-src ${webview.cspSource} data:`,
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
    <title>Typst Preview</title>
  </head>
  <body class="typst-preview">
    <div id="chrome" class="chrome">
      <button id="zoom-out" title="Zoom out">−</button>
      <span id="zoom-level">100%</span>
      <button id="zoom-in" title="Zoom in">+</button>
      <button id="fit-width" title="Fit width">↔</button>
      <button id="fit-page" title="Fit page">⤢</button>
      <button id="invert" title="Invert colors">◐</button>
      <span class="separator"></span>
      <button id="edit-source" title="Edit — show the source in a text editor">✎</button>
      <button id="export" title="Export… — PDF, SVG, PNG, or HTML">⭳</button>
      <span class="spacer"></span>
      <input id="go-to-page" type="number" min="1" value="1" title="Go to page" />
      <span id="page-count">/ 0</span>
    </div>
    <div id="status" class="status" hidden></div>
    <div id="pages" class="pages" tabindex="0"></div>
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
