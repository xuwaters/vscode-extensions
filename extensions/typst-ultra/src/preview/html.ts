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
      <input
        id="zoom-level"
        type="text"
        inputmode="decimal"
        value="100%"
        title="Zoom — type a percentage and press Enter"
        aria-label="Zoom percentage"
      />
      <button id="zoom-in" title="Zoom in">+</button>
      <button id="fit-width" title="Fit width">↔</button>
      <button id="fit-page" title="Fit page">⤢</button>
      <button id="invert" title="Invert colors">◐</button>
      <span class="separator"></span>
      <button id="edit-source" title="Edit — show the source in a text editor">✎</button>
      <button id="export" title="Export… — PDF, SVG, PNG, or HTML" aria-label="Export">
        <!-- Inline SVG, not a glyph: the download arrows live in Miscellaneous
             Symbols and Arrows, which the system UI fonts do not cover, so the
             button rendered as tofu. Inline markup is document, not a fetch, so
             the CSP does not apply to it. -->
        <svg class="icon" viewBox="0 0 16 16" aria-hidden="true" focusable="false">
          <path
            d="M8 2v7.5M5 6.6 8 9.7l3-3.1M3 11.4v2.1h10v-2.1"
            fill="none"
            stroke="currentColor"
            stroke-width="1.3"
            stroke-linecap="round"
            stroke-linejoin="round"
          />
        </svg>
      </button>
      <span class="spacer"></span>
      <!-- Labelled, because a bare number box next to "/ 12" reads as decoration
           rather than as somewhere to type. -->
      <label class="field" for="go-to-page">Page</label>
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
