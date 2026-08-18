import * as pdfjs from 'pdfjs-dist';
import type { DocumentInitParameters } from 'pdfjs-dist/types/src/display/api.js';

export type Pdfjs = typeof pdfjs;

/**
 * Where pdf.js's out-of-bundle data lives, as webview URLs. Handed over by the
 * host, which is the only side that can turn an extension path into one.
 */
export interface PdfAssets {
  worker: string;
  cMap: string;
  standardFont: string;
  wasm: string;
}

let workerBoot: Promise<void> | null = null;

/**
 * Start pdf.js's worker.
 *
 * The worker cannot be constructed from the extension's own URL: a worker
 * script must be same-origin, and a `vscode-resource` URL is not the webview's
 * origin. So the page fetches the script — which CORS does allow — and builds a
 * blob URL from it, which is same-origin by definition. That blob is why the
 * worker bundle is an IIFE rather than a module: a blob has no base URL for an
 * `import` to resolve against.
 *
 * If the worker cannot be created at all, pdf.js has its own fallback — it runs
 * the same code on the main thread — and `workerSrc` is left pointing at the
 * blob so that fallback has something to load. A blocked worker is then a slow
 * viewer rather than a blank one.
 */
export async function bootWorker(workerUrl: string): Promise<void> {
  workerBoot ??= (async () => {
    let blobUrl: string;
    try {
      const response = await fetch(workerUrl);
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      const source = await response.blob();
      blobUrl = URL.createObjectURL(source);
    } catch {
      // Nothing left to try but pdf.js's own resolution of the raw URL.
      pdfjs.GlobalWorkerOptions.workerSrc = workerUrl;
      return;
    }

    pdfjs.GlobalWorkerOptions.workerSrc = blobUrl;
    try {
      pdfjs.GlobalWorkerOptions.workerPort = new Worker(blobUrl);
    } catch {
      // Left to pdf.js, which will try the same URL and then the main thread.
    }
  })();
  return workerBoot;
}

/**
 * The parameters every document is opened with.
 *
 * The security posture is the same one the CSP describes, restated where pdf.js
 * can act on it:
 *
 * * `enableXfa: false` — XFA is a forms engine with its own scripting model.
 *   Nothing in a viewer needs it, and the bundle deliberately ships without the
 *   interpreter that would run it.
 * * No annotation *layer* is mounted and no scripting layer exists, so a
 *   hostile document has no widget that could act on its own. Links are read
 *   out of the annotations and rebuilt by us, which is what keeps `openExternal`
 *   behind the host's allow-list.
 * * `useWorkerFetch: false` — cMap, standard-font and wasm data are fetched on
 *   the main thread and handed to the worker. The worker is a blob and its
 *   requests are cross-origin to the extension's resource server; fetching from
 *   the page, which is the origin the CSP was written for, is the path that is
 *   actually guaranteed to work.
 * * `iccUrl` is deliberately unset. Reading it uses a *synchronous* XHR from
 *   inside the worker, which is the one request that cannot be routed through
 *   the main thread; without it pdf.js falls back to its built-in CMYK
 *   conversion, which is what every pdf.js integration did until recently.
 */
export function documentParams(
  source: { data: Uint8Array } | { url: string },
  assets: PdfAssets,
): DocumentInitParameters {
  return {
    ...source,
    enableXfa: false,
    cMapUrl: assets.cMap,
    cMapPacked: true,
    standardFontDataUrl: assets.standardFont,
    wasmUrl: assets.wasm,
    useWorkerFetch: false,
    // A document that fails halfway is still worth showing: the pages that did
    // parse render, and the reader sees the rest as blanks rather than an error.
    stopAtErrors: false,
  };
}

export { pdfjs };
