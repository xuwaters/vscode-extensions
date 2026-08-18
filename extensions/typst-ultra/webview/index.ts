import type {
  HostToWebview,
  PreviewPlace,
  WebviewToHost,
} from '../src/preview/messages.js';
import { TYPST_PREVIEW_TAG, TypstPreview } from './viewer/element.js';

/**
 * The webview's bootstrap, and the bundle's only entry point.
 *
 * Everything the reader interacts with is `<typst-preview>`; this file is the
 * wire between it and the extension host, and nothing else. Keeping the element
 * free of `acquireVsCodeApi` is what lets it be mounted in a test.
 *
 * The layers below depend only downwards:
 *
 * * `viewer/` — the FAST element, its template and its stylesheet. Reactive;
 *   everything the reader can see the state of.
 * * `render/` — the DOM. The virtualized page column, and the sanitizer every
 *   page passes through on its way into it. No state the reader sees.
 * * `model/`  — no DOM at all. Zoom arithmetic, fits, and reading what the
 *   reader typed into the zoom box. Where most of the tests are.
 */

interface VsCodeApi {
  postMessage(message: WebviewToHost): void;
  getState(): PreviewPlace | undefined;
  setState(state: PreviewPlace): void;
}

declare function acquireVsCodeApi(): VsCodeApi;

const api = acquireVsCodeApi();

// The tag is in the page's markup (see `src/preview/html.ts`); all that is left
// here is to wait for it to become an element. `@customElement` in fast-element
// 3 registers the name asynchronously, so until the definition lands the tag in
// the document is an inert `HTMLElement` with none of this class's methods on
// it — and constructing the class instead of waiting throws `Illegal
// constructor` outright.
//
// The `instanceof` is also what pulls the registration into the bundle: a
// tree-shaker has no way to know a decorator had a side effect.
await customElements.whenDefined(TYPST_PREVIEW_TAG);

const preview = document.querySelector(TYPST_PREVIEW_TAG);
if (!(preview instanceof TypstPreview)) {
  throw new Error(`the preview has no upgraded <${TYPST_PREVIEW_TAG}> to mount into`);
}

preview.host = {
  post: (message) => api.postMessage(message),
  save: (place) => {
    // Twice over, on purpose. `setState` survives this panel being reloaded;
    // the message is what a *different* surface — the tab a mode switch opens
    // next — is handed when it starts up.
    api.setState(place);
    api.postMessage({ type: 'state', ...place });
  },
};

// This panel's own memory, which beats the host's: it describes the panel the
// reader is looking at rather than the last one they used.
const saved = api.getState();
if (saved) preview.restore(saved, { override: true });

window.addEventListener('message', (event: MessageEvent<unknown>) => {
  const message = event.data as HostToWebview;
  if (typeof message !== 'object' || message === null) return;
  try {
    preview.handle(message);
  } catch (error) {
    api.postMessage({
      type: 'error',
      message: error instanceof Error ? error.message : String(error),
      context: message.type ?? 'unknown',
    });
  }
});

api.postMessage({ type: 'ready' });
