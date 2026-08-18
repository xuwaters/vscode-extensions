import * as vscode from 'vscode';

/**
 * The webview document.
 *
 * There is almost nothing in it: the preview is `<typst-preview>`, a FAST
 * element that renders its own chrome, so the only markup the host owns is the
 * empty tag, the two rules that size it, and a policy.
 *
 * The tag is written here rather than constructed by the bootstrap because
 * fast-element 3 defines a custom element *asynchronously*: a script that
 * constructs the class the moment its module has evaluated gets `Illegal
 * constructor`, because the name is not registered yet. Markup has no such
 * problem — the parser makes an ordinary unknown element and the browser
 * upgrades it in place once the definition lands.
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
    <style nonce="${nonce}">
      html,
      body {
        height: 100%;
        margin: 0;
        padding: 0;
        overflow: hidden;
      }
    </style>
  </head>
  <body>
    <typst-preview></typst-preview>
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
