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
  pdfjs: {
    /** Enough of pdf.js's text layer to be constructed and awaited. */
    TextLayer: class {
      textDivs: HTMLElement[] = [];
      constructor(_options: { container: HTMLElement }) {}
      async render(): Promise<void> {}
    },
  },
  bootWorker: async () => {},
  documentParams: (source: unknown) => source,
}));

const { PageColumn } = await import('./pageColumn.js');
const { viewportScale } = await import('../model/layout.js');
type ColumnOptions = ConstructorParameters<typeof PageColumn>[3];

/**
 * How many renders a document has going at once, and the worst it ever got.
 * Two on one canvas is the defect this counts: pdf.js refuses the second
 * outright, and the two wipe each other's output on the way down.
 */
interface Renders {
  /** Every render ever started. */
  calls: number;
  live: number;
  peak: number;
}

/**
 * Just enough document for `open` — every page is US Letter.
 *
 * Given a {@link Renders}, pages also rasterize: `render` returns a task that
 * stays in flight until it is cancelled, which is what a real one does for
 * long enough to matter and what the column's bookkeeping has to survive.
 */
function fakeDoc(numPages: number, renders?: Renders): PDFDocumentProxy {
  const render = (): unknown => {
    if (!renders) throw new Error('this document does not rasterize');
    renders.calls += 1;
    renders.live += 1;
    renders.peak = Math.max(renders.peak, renders.live);
    let stop: (reason: Error) => void = () => {};
    const promise = new Promise<void>((_resolve, reject) => {
      stop = reject;
    });
    promise.catch(() => {});
    return {
      promise,
      cancel: () => {
        renders.live -= 1;
        stop(new Error('RenderingCancelledException'));
      },
    };
  };

  return {
    numPages,
    getPage: async () => ({
      rotate: 0,
      getViewport: () => ({ width: 612, height: 792 }),
      render,
    }),
  } as unknown as PDFDocumentProxy;
}

/** Let every pending microtask and timer settle. */
const flush = (): Promise<void> => new Promise((resolve) => setTimeout(resolve, 0));

/**
 * Wait out the pause the column takes before it redraws a stretched page —
 * what turns a drag-resize into one rasterization rather than one per frame —
 * and then let the draw it starts run.
 */
async function settled(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 200));
  await flush();
}

/**
 * A scroller with a box.
 *
 * happy-dom lays nothing out, so every element it makes measures zero — which
 * the column reads, correctly, as a tab that is not in front, and refuses to
 * lay out against. Stating a size here is what tells it otherwise.
 */
function sized(el: HTMLElement, w: number, h: number): HTMLElement {
  Object.defineProperty(el, 'clientWidth', { value: w, configurable: true });
  Object.defineProperty(el, 'clientHeight', { value: h, configurable: true });
  return el;
}

function mount(
  view: { w: number; h: number } = { w: 1000, h: 800 },
  options: Partial<ColumnOptions> = {},
): { scroll: HTMLElement; host: HTMLElement; column: InstanceType<typeof PageColumn> } {
  const scroll = sized(document.createElement('div'), view.w, view.h);
  const host = document.createElement('div');
  scroll.append(host);
  document.body.replaceChildren(scroll);
  const column = new PageColumn(
    scroll,
    host,
    { onPage: () => {}, onLink: () => {}, onTextLayer: () => {} },
    { textLayer: false, links: false, maxCanvasPixels: 1 << 20, renderAhead: 0, ...options },
  );
  return { scroll, host, column };
}

/**
 * Open a document and lay it out — the two steps the element takes in order,
 * because the second one needs the page geometry the first one measures.
 */
async function opened(
  column: InstanceType<typeof PageColumn>,
  doc: PDFDocumentProxy,
  zoom = 1,
): Promise<void> {
  await column.open(doc);
  column.setView(zoom, 0);
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
    await opened(column, fakeDoc(3));
    const first = host.querySelector<HTMLElement>('.page');
    expect(first?.style.width).toBe('816px');
    expect(first?.style.height).toBe('1056px');
  });

  /**
   * The caller resolves its fit against the geometry `open` measures, so a
   * column that laid itself out first would rasterize a screen of pages at the
   * zoom it happened to be carrying only to throw the result away — and leave
   * that draw racing the one that replaced it.
   */
  it('leaves the layout to the caller rather than guessing a zoom', async () => {
    const { host, column } = mount();
    await column.open(fakeDoc(3));
    expect(host.querySelector<HTMLElement>('.page')?.style.width).toBe('');
  });

  it('replaces the previous document’s slots on a reopen', async () => {
    const { host, column } = mount();
    await column.open(fakeDoc(4));
    await column.open(fakeDoc(2));
    expect(host.querySelectorAll('.page')).toHaveLength(2);
  });
});

/**
 * A slot may have exactly one draw in flight. It is the whole of what `draw`
 * promises, and pdf.js depends on it: a second render against a canvas that
 * already has one is refused outright, and on the way down the two wipe each
 * other — the reader gets a page under its placeholder mask, or a black one
 * with a few runs of text on it, and nothing fixes it but a scroll or resize.
 *
 * The window is every document's first moment on screen: the column is laid
 * out once, then again as soon as the fit resolves against the geometry it
 * just measured. An abandoned draw finishing across that used to report that
 * no draw was running while one was.
 */
describe('one draw per slot', () => {
  /**
   * Two zooms, so two renders — the one the column was opened at and the one
   * the fit resolved to. A third is the abandoned draw's doing: it reported the
   * slot idle on its way out, and the next pass over the viewport believed it.
   */
  it('starts no draw the zoom did not ask for', async () => {
    const renders: Renders = { calls: 0, live: 0, peak: 0 };
    const { column } = mount();

    await opened(column, fakeDoc(1, renders));
    // The fit lands on a different zoom while the first draw is still going.
    column.setView(1.43, 0);
    await flush();
    // Any pass over the viewport — a scroll, a jump, a resize — used to find
    // the slot claiming to be idle and start a second draw on the same canvas.
    column.goToPage(1);
    await flush();

    expect(renders.calls).toBe(2);
  });

  it('never has two renders live on one canvas', async () => {
    const renders: Renders = { calls: 0, live: 0, peak: 0 };
    const { column } = mount();

    await opened(column, fakeDoc(1, renders));
    column.setView(1.43, 0);
    await flush();
    column.goToPage(1);
    await flush();

    expect(renders.peak).toBeLessThanOrEqual(1);
  });
});

/**
 * A tab VSCode is keeping alive in the background is laid out at nothing, and
 * everything the column would do about that is wrong: a fit measured against a
 * zero-width scroller is a nonsense zoom, and a render started now stops part
 * way — pdf.js continues a display render on an animation frame, and a hidden
 * webview is given none. Which is what a page stuck dark and half-drawn is.
 */
describe('a tab that is not in front', () => {
  it('does not lay the column out against a scroller with no box', async () => {
    const { host, column } = mount({ w: 0, h: 0 });
    await opened(column, fakeDoc(3));
    const first = host.querySelector<HTMLElement>('.page');
    expect(first?.style.width).toBe('');
  });

  it('lays it out on the refresh that follows the tab coming back', async () => {
    const { scroll, host, column } = mount({ w: 0, h: 0 });
    await opened(column, fakeDoc(3));
    sized(scroll, 1000, 800);
    column.refresh();
    const first = host.querySelector<HTMLElement>('.page');
    expect(first?.style.width).toBe('816px');
    expect(first?.style.height).toBe('1056px');
  });

  /**
   * The refresh runs every time the tab comes back, which is often. A column
   * that is already right has to come through it untouched, or a click away
   * and back would cost the visible band a re-rasterization.
   */
  it('leaves a column that is already laid out alone', async () => {
    const { host, column } = mount();
    await opened(column, fakeDoc(2));
    const first = host.querySelector<HTMLElement>('.page');
    // A relayout would rewrite this; a refresh over a correct column must not.
    first!.style.width = '7px';
    column.refresh();
    expect(first?.style.width).toBe('7px');
  });
});

/**
 * A rotation is not a zoom, and a raster keyed only by zoom cannot tell the
 * difference. The box turns, the bitmap does not, and the stylesheet stretches
 * the old landscape image across the new portrait box — a page whose content is
 * skewed, at the right size, with nothing but another zoom to fix it.
 */
describe('rotating the pages', () => {
  /**
   * A document whose pages finish drawing, so a slot ends up *holding* a raster
   * rather than perpetually starting one. That is the state the defect lives in:
   * a page still mid-draw is released by any relayout whatever it was keyed on.
   */
  function drawnDoc(numPages: number, drawn: { calls: number }): PDFDocumentProxy {
    return {
      numPages,
      getPage: async () => ({
        rotate: 0,
        getViewport: ({ scale = 1 }: { scale?: number }) => ({
          width: 612 * scale,
          height: 792 * scale,
          scale,
        }),
        render: () => {
          drawn.calls += 1;
          return { promise: Promise.resolve(), cancel: () => {} };
        },
      }),
    } as unknown as PDFDocumentProxy;
  }

  it('turns every box a quarter turn', async () => {
    const { host, column } = mount();
    await opened(column, fakeDoc(2));
    column.setView(1, 90);
    const first = host.querySelector<HTMLElement>('.page');
    expect(first?.style.width).toBe('1056px');
    expect(first?.style.height).toBe('816px');
  });

  it('draws a page that is already drawn again, though the zoom did not move', async () => {
    const drawn = { calls: 0 };
    const { column } = mount();
    await opened(column, drawnDoc(1, drawn));
    await flush();
    expect(drawn.calls).toBe(1);

    column.setView(1, 90);
    await flush();
    expect(drawn.calls).toBe(2);
  });

  it('leaves a page that is already the right way up alone', async () => {
    const drawn = { calls: 0 };
    const { column } = mount();
    await opened(column, drawnDoc(1, drawn));
    await flush();
    column.setView(1, 90);
    await flush();
    // The same view again: a scroll, a resize, a tab coming back to the front.
    column.setView(1, 90);
    column.goToPage(1);
    await flush();
    expect(drawn.calls).toBe(2);
  });
});

describe('dual-column mode', () => {
  it('groups the pages into rows of two', async () => {
    const { host, column } = mount();
    await opened(column, fakeDoc(5));
    column.setMode('dual');
    const rows = [...host.querySelectorAll<HTMLElement>('.spread')];
    expect(rows).toHaveLength(3);
    expect(rows.map((row) => row.querySelectorAll('.page').length)).toEqual([2, 2, 1]);
  });

  it('lays the pages of a row out level with each other', async () => {
    const { scroll, column } = mount();
    await opened(column, fakeDoc(4));
    column.setMode('dual');
    column.goToPage(3);
    // Page 4 sits beside page 3, so scrolling to either lands in the same place.
    const beside = scroll.scrollTop;
    column.goToPage(4);
    expect(scroll.scrollTop).toBe(beside);
  });

  it('puts the pages back in one column when it leaves', async () => {
    const { host, column } = mount();
    await opened(column, fakeDoc(4));
    column.setMode('dual');
    column.setMode('continuous');
    expect(host.querySelectorAll('.spread')).toHaveLength(0);
    const children = [...host.children];
    expect(children).toHaveLength(4);
    expect(children.every((child) => child.classList.contains('page'))).toBe(true);
  });

  it('keeps the reader on the page they were on across the switch', async () => {
    const { column } = mount();
    await opened(column, fakeDoc(8));
    column.goToPage(5);
    column.setMode('dual');
    expect(column.page).toBe(5);
    column.setMode('continuous');
    expect(column.page).toBe(5);
  });
});

describe('single-page mode', () => {
  it('takes every page but the current one out of the flow', async () => {
    const { host, column } = mount();
    await opened(column, fakeDoc(4));
    column.setMode('single');
    const pages = [...host.querySelectorAll<HTMLElement>('.page')];
    expect(pages.map((page) => page.style.display)).toEqual(['', 'none', 'none', 'none']);
  });

  it('swaps which page is in the flow rather than scrolling to it', async () => {
    const { scroll, host, column } = mount();
    await opened(column, fakeDoc(4));
    column.setMode('single');
    column.goToPage(3);
    const pages = [...host.querySelectorAll<HTMLElement>('.page')];
    expect(pages.map((page) => page.style.display)).toEqual(['none', 'none', '', 'none']);
    expect(column.page).toBe(3);
    expect(scroll.scrollTop).toBe(0);
  });

  it('puts every page back when it returns to continuous', async () => {
    const { host, column } = mount();
    await opened(column, fakeDoc(3));
    column.setMode('single');
    column.setMode('continuous');
    const pages = [...host.querySelectorAll<HTMLElement>('.page')];
    expect(pages.map((page) => page.style.display)).toEqual(['', '', '']);
  });

  it('keeps the reader on the page they were on across the switch', async () => {
    const { column } = mount();
    await opened(column, fakeDoc(5));
    column.goToPage(4);
    column.setMode('single');
    expect(column.page).toBe(4);
    column.setMode('continuous');
    expect(column.page).toBe(4);
  });
});

/**
 * A selection is anchored in the nodes of the text layer, so releasing one out
 * from under the reader collapses it — and in a virtualized column the page a
 * selection starts on leaves the render band the moment the drag reaches the
 * bottom of the screen. Dragging across a page boundary would then select
 * nothing at all.
 */
describe('a selection in flight', () => {
  /** Put a text layer on a page and select a word of it. */
  function selectOn(host: HTMLElement, page: number): HTMLElement {
    const text = [...host.querySelectorAll<HTMLElement>('.page-text')][page - 1]!;
    const span = document.createElement('span');
    span.textContent = 'selected';
    text.append(span);
    const range = document.createRange();
    range.selectNodeContents(span);
    const selection = document.getSelection()!;
    selection.removeAllRanges();
    selection.addRange(range);
    return text;
  }

  it('survives the page it started on scrolling out of the band', async () => {
    const { host, column } = mount();
    await opened(column, fakeDoc(5, { calls: 0, live: 0, peak: 0 }));
    const text = selectOn(host, 1);
    column.goToPage(5);
    expect(text.childNodes).toHaveLength(1);
  });

  it('does not keep the layers of pages it does not reach into', async () => {
    const { host, column } = mount();
    await opened(column, fakeDoc(5, { calls: 0, live: 0, peak: 0 }));
    const text = selectOn(host, 1);
    document.getSelection()?.removeAllRanges();
    column.goToPage(5);
    expect(text.childNodes).toHaveLength(0);
  });

  /**
   * A relayout is the release a selection cannot be spared: the boxes have
   * moved, so glyph runs left standing would sit at the old zoom and highlight
   * the wrong part of the page.
   */
  it('is not spared by a relayout, where the boxes have genuinely moved', async () => {
    const { host, column } = mount();
    await opened(column, fakeDoc(5, { calls: 0, live: 0, peak: 0 }));
    const text = selectOn(host, 1);
    column.setView(2, 0);
    expect(text.childNodes).toHaveLength(0);
  });
});

/**
 * pdf.js writes each run's height in PDF units and leaves the stylesheet to
 * turn that into a font size, through `--total-scale-factor`. Set the wrong
 * variable — `--scale-factor`, its name before pdf.js 5 — and nothing reads it:
 * every run falls back to the font it inherits from the viewer's chrome, the
 * glyph boxes come out at the wrong width, and the selection the reader drags
 * is painted across the page nowhere near the words it covers.
 */
describe('the text layer', () => {
  /** A document whose pages rasterize and carry text. */
  function textDoc(numPages: number): PDFDocumentProxy {
    return {
      numPages,
      getPage: async () => ({
        rotate: 0,
        getViewport: ({ scale = 1 }: { scale?: number }) => ({
          width: 612 * scale,
          height: 792 * scale,
          scale,
        }),
        render: () => ({ promise: Promise.resolve(), cancel: () => {} }),
        streamTextContent: () => ({}),
        getAnnotations: async () => [],
      }),
    } as unknown as PDFDocumentProxy;
  }

  it('scales the runs by the viewport the page was drawn at', async () => {
    const { host, column } = mount({ w: 1000, h: 800 }, { textLayer: true });
    await opened(column, textDoc(1), 1.25);
    await flush();
    const text = host.querySelector<HTMLElement>('.page-text');
    expect(text?.style.getPropertyValue('--total-scale-factor')).toBe(
      String(viewportScale(1.25)),
    );
  });

  it('leaves nothing behind under the name pdf.js stopped reading', async () => {
    const { host, column } = mount({ w: 1000, h: 800 }, { textLayer: true });
    await opened(column, textDoc(1));
    await flush();
    const text = host.querySelector<HTMLElement>('.page-text');
    expect(text?.style.getPropertyValue('--scale-factor')).toBe('');
  });
});

/**
 * Dragging a tab wider is a new scroller size every animation frame, and a fit
 * turns every one of those into a new zoom — so a resize is not one relayout,
 * it is sixty a second for as long as the reader holds the mouse down.
 *
 * Releasing the rasters on each of those is what made the pages flicker: every
 * frame blanked the visible pages down to their placeholder and started a
 * render that the next frame cancelled before it could reach the screen. The
 * whole gesture was spent looking at empty grey rectangles.
 */
describe('a view that is still moving', () => {
  /** A document whose pages finish drawing the moment they are asked to. */
  function drawnDoc(numPages: number, drawn: { calls: number }): PDFDocumentProxy {
    return {
      numPages,
      getPage: async () => ({
        rotate: 0,
        getViewport: ({ scale = 1 }: { scale?: number }) => ({
          width: 612 * scale,
          height: 792 * scale,
          scale,
        }),
        render: () => {
          drawn.calls += 1;
          return { promise: Promise.resolve(), cancel: () => {} };
        },
      }),
    } as unknown as PDFDocumentProxy;
  }

  /**
   * A document whose renders finish only when the test says so — which is the
   * window that matters here: the seconds a page spends being redrawn are the
   * seconds the reader must still have something to look at.
   */
  function gatedDoc(numPages: number, waiting: Array<() => void>): PDFDocumentProxy {
    return {
      numPages,
      getPage: async () => ({
        rotate: 0,
        getViewport: ({ scale = 1 }: { scale?: number }) => ({
          width: 612 * scale,
          height: 792 * scale,
          scale,
        }),
        render: () => {
          let done = (): void => {};
          const promise = new Promise<void>((resolve) => {
            done = resolve;
          });
          waiting.push(done);
          return { promise, cancel: () => {} };
        },
      }),
    } as unknown as PDFDocumentProxy;
  }

  /** One frame of a drag: the scroller is narrower, so the fit is smaller. */
  function drag(
    scroll: HTMLElement,
    column: InstanceType<typeof PageColumn>,
    width: number,
  ): void {
    sized(scroll, width, 800);
    column.setView(width / 1000, 0);
  }

  const canvasOf = (host: HTMLElement): HTMLCanvasElement =>
    host.querySelector<HTMLCanvasElement>('.page-canvas')!;

  it('keeps the raster on the page it is stretching rather than blanking it', async () => {
    const drawn = { calls: 0 };
    const { scroll, host, column } = mount();
    await opened(column, drawnDoc(1, drawn));
    await flush();
    const before = canvasOf(host);
    expect(before.width).toBe(816);

    drag(scroll, column, 700);

    expect(canvasOf(host)).toBe(before);
    expect(before.width).toBe(816);
    expect(host.querySelector('.page')?.classList.contains('placeholder')).toBe(false);
  });

  it('rasterizes once the drag has stopped, not once per frame of it', async () => {
    const drawn = { calls: 0 };
    const { scroll, column } = mount();
    await opened(column, drawnDoc(1, drawn));
    await flush();
    expect(drawn.calls).toBe(1);

    for (const width of [980, 940, 900, 860, 820]) {
      drag(scroll, column, width);
      await flush();
    }
    expect(drawn.calls).toBe(1);

    await settled();
    expect(drawn.calls).toBe(2);
  });

  /**
   * The wait is for pages that have something to show. A page that has never
   * been drawn has nothing, so scrolling ahead during a drag must still fill
   * the band rather than leave the reader on a placeholder until they let go.
   */
  it('still draws a page that has no raster to stretch', async () => {
    const drawn = { calls: 0 };
    const { scroll, column } = mount();
    await opened(column, drawnDoc(5, drawn));
    await flush();
    const before = drawn.calls;

    drag(scroll, column, 900);
    column.goToPage(4);
    await flush();

    expect(drawn.calls).toBeGreaterThan(before);
  });

  /**
   * Sizing a canvas clears it, so a draw that paints into the page's own canvas
   * blanks it for the whole of the render — the flash the reader sees at the
   * end of every resize, however few renders it took to get there.
   */
  it('swaps a finished raster in rather than emptying the page to make one', async () => {
    const waiting: Array<() => void> = [];
    const { scroll, host, column } = mount();
    await opened(column, gatedDoc(1, waiting));
    await flush();
    waiting.shift()?.();
    await flush();
    const before = canvasOf(host);
    expect(before.width).toBe(816);

    drag(scroll, column, 700);
    await settled();
    // The redraw is under way, and the page is still the one the reader had.
    expect(waiting).toHaveLength(1);
    expect(canvasOf(host)).toBe(before);
    expect(before.width).toBe(816);

    waiting.shift()?.();
    await flush();
    const after = canvasOf(host);
    expect(after).not.toBe(before);
    expect(after.width).toBe(571);
    // And the raster it replaced has handed its bitmap back.
    expect(before.width).toBe(0);
  });
});
