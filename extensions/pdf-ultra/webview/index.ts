import type { HostToWebview, WebviewToHost } from '../src/messages.js';
import { PDF_VIEWER_TAG, PdfViewer } from './viewer/element.js';

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

// The tag is in the page's markup (see `src/html.ts`); all that is left here is
// to wait for it to become an element. `@customElement` in fast-element 3
// registers the name asynchronously, so until the definition lands the tag in
// the document is an inert `HTMLElement` with none of this class's methods on
// it — and constructing the class instead of waiting throws `Illegal
// constructor` outright.
//
// The `instanceof` is also what pulls the registration into the bundle: a
// tree-shaker has no way to know a decorator had a side effect.
await customElements.whenDefined(PDF_VIEWER_TAG);

const viewer = document.querySelector(PDF_VIEWER_TAG);
if (!(viewer instanceof PdfViewer)) {
  throw new Error(`the page has no upgraded <${PDF_VIEWER_TAG}> to mount into`);
}

viewer.host = { post: (message) => api.postMessage(message) };

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
