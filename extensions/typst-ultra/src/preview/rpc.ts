import * as vscode from 'vscode';
import type { Client } from '../lsp/client.js';
import * as config from '../config.js';
import type { PageMetric, PagePatch } from './messages.js';

/**
 * The preview's half of the LSP surface.
 *
 * Both preview surfaces — the following panel and the full-tab preview editor —
 * ask the server the same three questions, so they ask them from here rather
 * than each keeping its own copy of the parameter shapes.
 */

/** The server's answer to `typst/documentMetrics`. */
export interface MetricsResult {
  pageCount: number;
  pages: PageMetric[];
}

/** The server's answer to `typst/renderPages`. */
export interface RenderResult {
  patches: PagePatch[];
  pageCount: number;
}

/** The server's answer to `typst/jumpFromClick`. */
export type JumpResult =
  | {
      kind: 'source';
      uri: string;
      position: { line: number; character: number };
    }
  | { kind: 'url'; url: string }
  | { kind: 'page'; page: number; xPt: number; yPt: number };

/**
 * Make `uri` the document the server compiles, and compile it now.
 *
 * The server follows whichever file was last opened, changed, or saved. That is
 * right while typing and wrong the moment the reader clicks a second `.typ`
 * that is already open: no edit arrives, so nothing tells the server the
 * subject changed, and the preview would measure the *previous* document. A
 * settled compile root — a pin or `typstUltra.mainFile` — outranks this, and
 * the server ignores the URI in that case, but we do not even ask: recompiling
 * a book to switch chapters is a waste of the only thread the engine has.
 */
export function compileNow(client: Client, uri: vscode.Uri, fixed: boolean): void {
  if (fixed) return;
  client.notify('typst/compile', { uri: uri.toString() });
}

export function fetchMetrics(
  client: Client,
  uri: vscode.Uri,
): Promise<MetricsResult | undefined> {
  return client.request<MetricsResult>('typst/documentMetrics', {
    uri: uri.toString(),
  });
}

export function fetchPages(
  client: Client,
  uri: vscode.Uri,
  first: number,
  last: number,
  known: Record<number, string>,
  zoom: number,
): Promise<RenderResult | undefined> {
  const pages: number[] = [];
  for (let index = first; index <= last; index += 1) pages.push(index);

  const mode = config.read(uri).host.preview.renderMode;
  return client.request<RenderResult>('typst/renderPages', {
    uri: uri.toString(),
    pages,
    knownHashes: known,
    mode,
    // A raster page is baked at one resolution, so it has to be rendered for
    // the zoom it will be shown at. 96 dpi is 1:1 with a CSS pixel.
    ppi: mode === 'svg' ? undefined : Math.min(600, Math.max(72, 96 * zoom)),
  });
}

export function jumpFromClick(
  client: Client,
  page: number,
  xPt: number,
  yPt: number,
): Promise<JumpResult | null | undefined> {
  return client.request<JumpResult | null>('typst/jumpFromClick', {
    page,
    xPt,
    yPt,
  });
}
