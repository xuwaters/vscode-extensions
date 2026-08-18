import * as vscode from 'vscode';

/**
 * The webview document.
 *
 * There is almost nothing in it: the viewer is `<pdf-viewer>`, a FAST element
 * that renders its own chrome into its own shadow root, so the only markup the
 * host owns is the shell it mounts into. The two rules below are the page's
 * whole contribution — a document that fills the tab, and a policy.
 *
 * The CSP is the load-bearing part, because a PDF is an untrusted document that
 * can come from anywhere and pdf.js is a large parser sitting between it and
 * the page:
 *
 * * `default-src 'none'` — nothing loads unless named below.
 * * `script-src 'nonce-…' 'wasm-unsafe-eval'` — no inline or injected script
 *   can run, because no `script-src` value permits one. `'wasm-unsafe-eval'` is
 *   the narrow allowance that lets `WebAssembly.instantiate` compile pdf.js's
 *   JBIG2, JPEG 2000 and ICC decoders; it does **not** re-enable `eval`.
 *   Without it those decoders fall back to a dynamic `import()` of a JavaScript
 *   build, which is cross-origin here and fails — so scanned documents would
 *   not render.
 * * `worker-src blob:` — the pdf.js worker, constructed from a blob of a script
 *   the page fetched from this extension (see `webview/pdfjs.ts`). A worker
 *   cannot be constructed from a `vscode-resource` URL directly: worker scripts
 *   must be same-origin and that URL is not.
 * * `connect-src` is this extension's resources and the document's own folder —
 *   the PDF's bytes, the worker script, and pdf.js's cMap/font/wasm data all
 *   arrive by `fetch`. `blob:` is there because the worker inherits this policy.
 * * `img-src` allows `data:` and `blob:` because that is how an embedded raster
 *   image reaches the DOM; `font-src data:` because pdf.js installs a
 *   document's embedded fonts as data URIs.
 * * No remote origins at all: no CDN, no web fonts, no telemetry. A document
 *   that names one gets nothing.
 */
export function html(webview: vscode.Webview, extensionUri: vscode.Uri): string {
  const nonce = makeNonce();
  const script = webview.asWebviewUri(
    vscode.Uri.joinPath(extensionUri, 'dist', 'webview.js'),
  );

  const csp = [
    `default-src 'none'`,
    `img-src ${webview.cspSource} data: blob:`,
    `style-src ${webview.cspSource} 'unsafe-inline'`,
    `script-src 'nonce-${nonce}' 'wasm-unsafe-eval'`,
    `font-src ${webview.cspSource} data:`,
    `connect-src ${webview.cspSource} blob: data:`,
    `worker-src blob:`,
  ].join('; ');

  return `<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta http-equiv="Content-Security-Policy" content="${csp}" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>PDF Ultra</title>
    <style nonce="${nonce}">
      html,
      body {
        height: 100%;
        margin: 0;
        padding: 0;
        overflow: hidden;
      }
      pdf-viewer {
        display: block;
        height: 100%;
      }
    </style>
  </head>
  <body>
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
