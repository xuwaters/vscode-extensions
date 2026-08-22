import type { HostToWebview, WebviewToHost } from '../src/messages.js';
import { CSV_GRID_TAG, CsvGrid } from './viewer/element.js';

/**
 * The webview's bootstrap, and the bundle's only entry point.
 *
 * Everything the reader interacts with is `<csv-grid>`; this file is the wire
 * between it and the extension host, and nothing else. Keeping the element free
 * of `acquireVsCodeApi` is what lets it be mounted in a test.
 *
 * The layers below are pdf-ultra's and typst-ultra's, and depend only downwards:
 *
 * * `viewer/` — the FAST element, its template and its stylesheet. Reactive:
 *   everything the reader can see the *state* of, and every decision about what
 *   a gesture means.
 * * `render/` — the DOM and nothing reactive. `Sheet` owns the scrolling body:
 *   a few hundred recycled boxes over a table of any size, repositioned every
 *   frame. It reports gestures; it decides nothing.
 * * `model/`  — no DOM at all. Selection, virtualization arithmetic, find, the
 *   clipboard. Where most of the tests are.
 * * `../src/csv/` — shared with the extension host: one parser and one writer,
 *   so the table on screen and the bytes on disk cannot disagree about the
 *   file. It lives under `src/` because that is the folder the packaging step
 *   already knows to leave out of the VSIX.
 */

interface VsCodeApi {
  postMessage(message: WebviewToHost): void;
}

declare function acquireVsCodeApi(): VsCodeApi;

const api = acquireVsCodeApi();

// The tag is in the page's markup (see `src/editor/html.ts`); all that is left
// here is to wait for it to become an element. `@customElement` in fast-element 3
// registers the name asynchronously, so until the definition lands the tag in
// the document is an inert `HTMLElement` with none of this class's methods on
// it — and constructing the class instead of waiting throws `Illegal
// constructor` outright.
//
// The `instanceof` is also what pulls the registration into the bundle: a
// tree-shaker has no way to know a decorator had a side effect.
await customElements.whenDefined(CSV_GRID_TAG);

const grid = document.querySelector(CSV_GRID_TAG);
if (!(grid instanceof CsvGrid)) {
  throw new Error(`the page has no upgraded <${CSV_GRID_TAG}> to mount into`);
}

grid.host = { post: (message) => api.postMessage(message) };

window.addEventListener('message', (event: MessageEvent<unknown>) => {
  const message = event.data as HostToWebview;
  if (typeof message !== 'object' || message === null) return;
  try {
    grid.handle(message);
  } catch (error) {
    api.postMessage({
      type: 'error',
      message: error instanceof Error ? error.message : String(error),
      context: message.type ?? 'unknown',
    });
  }
});

api.postMessage({ type: 'ready' });
