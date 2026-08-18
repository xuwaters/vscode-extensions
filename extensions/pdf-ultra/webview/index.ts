import type { HostToWebview, WebviewToHost } from '../src/messages.js';
import { PdfViewer } from './viewer/element.js';

/**
 * The webview's bootstrap, and one of the two bundle entry points — the other
 * is `pdfWorker.ts`, which is why both sit at the top of this folder while
 * everything else is filed under a layer.
 *
 * Everything the reader interacts with is `<pdf-viewer>`; this file is the wire
 * between it and the extension host, and nothing else. Keeping the element free
 * of `acquireVsCodeApi` is what lets it be mounted in a test.
 *
 * The layers below depend only downwards:
 *
 * * `viewer/` — the FAST element and its controllers. Reactive; everything the
 *   reader can see the state of.
 * * `render/` — pdf.js and the DOM. The page column, the loader, destinations,
 *   find painting. No state the reader sees.
 * * `model/`  — no DOM and no pdf.js. Layout arithmetic, find, the outline
 *   tree, zoom parsing, the byte-transfer buffer. Where most of the tests are.
 */

interface VsCodeApi {
  postMessage(message: WebviewToHost): void;
}

declare function acquireVsCodeApi(): VsCodeApi;

const api = acquireVsCodeApi();

// Referencing the class is what pulls the `@customElement` registration into
// the bundle: a tree-shaker has no way to know a decorator had a side effect.
const viewer = new PdfViewer();
viewer.host = { post: (message) => api.postMessage(message) };
document.body.append(viewer);

window.addEventListener('message', (event: MessageEvent<unknown>) => {
  const message = event.data as HostToWebview;
  if (typeof message !== 'object' || message === null) return;
  try {
    viewer.handle(message);
  } catch (error) {
    api.postMessage({
      type: 'error',
      message: error instanceof Error ? error.message : String(error),
      context: message.type ?? 'unknown',
    });
  }
});

api.postMessage({ type: 'ready' });
