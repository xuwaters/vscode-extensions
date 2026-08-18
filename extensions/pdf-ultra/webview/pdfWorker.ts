/**
 * The pdf.js worker, as its own bundle.
 *
 * Nothing here but the import: the module's top-level code installs the worker's
 * message handler on `self`, which is the whole job. It is bundled to
 * `dist/pdf.worker.js` as an IIFE so that the page can turn it into a blob and
 * construct a same-origin worker from it — see `pdfjs.ts`.
 */
import 'pdfjs-dist/build/pdf.worker.min.mjs';
