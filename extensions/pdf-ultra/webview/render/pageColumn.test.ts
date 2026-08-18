// @vitest-environment happy-dom
import type { PDFDocumentProxy } from 'pdfjs-dist';
import { describe, expect, it, vi } from 'vitest';

/**
 * The column without a rasterizer.
 *
 * happy-dom has no canvas, so nothing here is ever drawn — but building the
 * slots is not drawing them, and that is the part a reader notices first: a
 * column with no boxes is a tab that opens, renders its toolbar, and shows an
 * empty page area with no error to explain it.
 */
vi.mock('./pdfjs.js', () => ({
  pdfjs: { TextLayer: class {} },
  bootWorker: async () => {},
  documentParams: (source: unknown) => source,
}));

const { PageColumn } = await import('./pageColumn.js');

/** Just enough document for `open` — every page is US Letter. */
function fakeDoc(numPages: number): PDFDocumentProxy {
  return {
    numPages,
    getPage: async () => ({
      rotate: 0,
      getViewport: () => ({ width: 612, height: 792 }),
    }),
  } as unknown as PDFDocumentProxy;
}

function mount(): { scroll: HTMLElement; host: HTMLElement; column: InstanceType<typeof PageColumn> } {
  const scroll = document.createElement('div');
  const host = document.createElement('div');
  scroll.append(host);
  document.body.replaceChildren(scroll);
  const column = new PageColumn(
    scroll,
    host,
    { onPage: () => {}, onLink: () => {}, onTextLayer: () => {} },
    { textLayer: false, links: false, maxCanvasPixels: 1 << 20, renderAhead: 0 },
  );
  return { scroll, host, column };
}

describe('taking a document', () => {
  /**
   * `teardown` bumps the generation itself, so a generation claimed before it
   * is already stale at the first `await` — and `open` abandons its own slot
   * build, silently, every single time.
   */
  it('builds a slot per page rather than abandoning itself as stale', async () => {
    const { host, column } = mount();
    await column.open(fakeDoc(7));
    expect(host.querySelectorAll('.page')).toHaveLength(7);
    expect(column.pageCount).toBe(7);
  });

  it('sizes every slot from the document rather than leaving it collapsed', async () => {
    const { host, column } = mount();
    await column.open(fakeDoc(3));
    const first = host.querySelector<HTMLElement>('.page');
    expect(first?.style.width).toBe('816px');
    expect(first?.style.height).toBe('1056px');
  });

  it('replaces the previous document’s slots on a reopen', async () => {
    const { host, column } = mount();
    await column.open(fakeDoc(4));
    await column.open(fakeDoc(2));
    expect(host.querySelectorAll('.page')).toHaveLength(2);
  });
});
