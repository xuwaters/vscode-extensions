// @vitest-environment happy-dom
import { describe, expect, it, vi } from 'vitest';
import { PageColumn } from './pageColumn.js';

interface Report {
  first: number;
  last: number;
  known: Record<number, string>;
}

const metric = (index: number, hash: string) => ({
  index,
  widthPt: 595,
  heightPt: 842,
  hash,
});

function build(): {
  column: PageColumn;
  container: HTMLElement;
  reports: Report[];
} {
  const container = document.createElement('div');
  document.body.replaceChildren(container);
  const reports: Report[] = [];
  const column = new PageColumn(container, {
    onViewport: (first, last, known) => reports.push({ first, last, known }),
    onClick: () => undefined,
  });
  return { column, container, reports };
}

/**
 * The panel follows the active editor, so one column outlives several
 * documents. Handing it a different document has to leave nothing of the old
 * one behind — not its pages, not its cache, and not its scroll position.
 */
describe('changing the document under the column', () => {
  it('drops every page and returns to the top', () => {
    const { column, container } = build();
    column.setMetrics([metric(0, 'a'.repeat(16)), metric(1, 'b'.repeat(16))]);
    expect(column.length).toBe(2);
    container.scrollTop = 400;

    column.reset();

    expect(column.length).toBe(0);
    expect(container.children).toHaveLength(0);
    expect(container.scrollTop).toBe(0);
  });

  it('leaves nothing for the next document to inherit by hash', () => {
    const { column, container, reports } = build();
    const hash = 'c'.repeat(16);
    column.setMetrics([metric(0, hash)]);
    column.applyPatches([
      {
        op: 'replace',
        index: 0,
        hash,
        format: 'svg',
        content: '<svg xmlns="http://www.w3.org/2000/svg"><g /></svg>',
      },
    ]);
    expect(container.querySelectorAll('svg')).toHaveLength(1);

    column.reset();
    // A new document can carry the same hash — an empty first page, say. It
    // must be fetched again rather than served from the cache of the document
    // that has just been replaced.
    column.setMetrics([metric(0, hash)]);

    expect(container.querySelectorAll('svg')).toHaveLength(0);
    expect(reports.at(-1)?.known).toEqual({});
  });
});

/**
 * The reason the preview does not flash on every keystroke. A page being edited
 * gets a new hash on each compile, and its replacement is a round trip away —
 * so what is already on screen has to stay there until it arrives.
 */
describe('an edited page', () => {
  const svg = '<svg xmlns="http://www.w3.org/2000/svg"><g /></svg>';
  const before = 'a'.repeat(16);
  const after = 'b'.repeat(16);

  function typed(): ReturnType<typeof build> {
    const built = build();
    built.column.setMetrics([metric(0, before)]);
    built.column.applyPatches([
      { op: 'replace', index: 0, hash: before, format: 'svg', content: svg },
    ]);
    // The keystroke: same page, new content, no rendering for it yet.
    built.column.setMetrics([metric(0, after)]);
    return built;
  }

  it('keeps showing what it had until the new page arrives', () => {
    const { container } = typed();
    expect(container.querySelectorAll('svg')).toHaveLength(1);
    expect(container.querySelector('.page')?.classList.contains('placeholder')).toBe(
      false,
    );
  });

  it('asks for the new page by telling the server what it is really showing', () => {
    const { reports } = typed();
    expect(reports.at(-1)?.known).toEqual({ 0: before });
  });

  it('leaves the element in place, so nothing is re-attached for nothing', () => {
    const { column, container } = typed();
    const element = container.firstElementChild;
    column.setMetrics([metric(0, 'c'.repeat(16))]);
    expect(container.firstElementChild).toBe(element);
  });

  it('swaps the page in one move when it does arrive', () => {
    const { column, container } = typed();
    column.applyPatches([
      { op: 'replace', index: 0, hash: after, format: 'svg', content: svg },
    ]);
    expect(container.querySelectorAll('svg')).toHaveLength(1);
    expect(container.querySelector('.page')?.classList.contains('placeholder')).toBe(
      false,
    );
  });
});

/**
 * Page identity is the hash, so a page that only moved keeps its element — and
 * therefore has to be told it has moved, or it prints the number it had before
 * and reports the wrong index when it is clicked.
 */
describe('a page that shifted', () => {
  it('takes its new number with it', () => {
    const { column, container } = build();
    const kept = 'a'.repeat(16);
    column.setMetrics([metric(0, kept)]);
    const element = container.firstElementChild as HTMLElement;

    column.setMetrics([metric(0, 'b'.repeat(16)), metric(1, kept)]);

    expect(container.children[1]).toBe(element);
    expect(element.dataset.index).toBe('1');
    expect(element.querySelector('.page-number')?.textContent).toBe('2');
  });
});

/**
 * Scrolling asks the column where every page is, and taking that off the DOM
 * flushes layout. Neither the measuring nor the reporting may happen per event:
 * a trackpad fires scrolls far faster than the screen is painted.
 */
describe('scrolling', () => {
  const frame = (): Promise<void> =>
    new Promise((resolve) => requestAnimationFrame(() => resolve()));

  it('reports the viewport once a frame, not once an event', async () => {
    const { column, container, reports } = build();
    column.setMetrics([metric(0, 'a'.repeat(16)), metric(1, 'b'.repeat(16))]);
    const before = reports.length;

    for (let i = 0; i < 5; i += 1) container.dispatchEvent(new Event('scroll'));
    expect(reports).toHaveLength(before);

    await frame();
    expect(reports).toHaveLength(before + 1);
  });

  it('measures the pages once and reads the measurement after that', async () => {
    const { column, container } = build();
    const reads = vi.spyOn(HTMLElement.prototype, 'offsetTop', 'get');
    try {
      column.setMetrics([metric(0, 'a'.repeat(16)), metric(1, 'b'.repeat(16))]);
      const measured = reads.mock.calls.length;
      expect(measured).toBeGreaterThan(0);

      container.dispatchEvent(new Event('scroll'));
      await frame();
      container.dispatchEvent(new Event('scroll'));
      await frame();

      // Scrolling moves no page, so it costs no layout reads at all.
      expect(reads.mock.calls.length).toBe(measured);
    } finally {
      reads.mockRestore();
    }
  });

  it('measures again once the pages have been rescaled', async () => {
    const { column, container } = build();
    column.setMetrics([metric(0, 'a'.repeat(16))]);
    container.dispatchEvent(new Event('scroll'));
    await frame();

    const reads = vi.spyOn(HTMLElement.prototype, 'offsetTop', 'get');
    try {
      column.setZoom(2);
      expect(reads.mock.calls.length).toBeGreaterThan(0);
    } finally {
      reads.mockRestore();
    }
  });
});

describe('sizing the pages', () => {
  it('reports the first page as the one a fit is measured against', () => {
    const { column } = build();
    expect(column.baseGeom).toBeNull();
    column.setMetrics([metric(0, 'd'.repeat(16))]);
    expect(column.baseGeom).toEqual({ widthPt: 595, heightPt: 842 });
  });

  it('scales every page box with the zoom', () => {
    const { column, container } = build();
    column.setMetrics([metric(0, 'e'.repeat(16))]);
    const page = container.firstElementChild as HTMLElement;
    const at100 = page.style.width;

    column.setZoom(2);

    expect(column.scale).toBe(2);
    // The box is a CSS length, so it comes back rounded to however many places
    // the engine serializes; the ratio is what the test is about.
    expect(Number.parseFloat(page.style.width)).toBeCloseTo(
      Number.parseFloat(at100) * 2,
      3,
    );
  });

  it('clamps a zoom that is out of range', () => {
    const { column } = build();
    column.setMetrics([metric(0, 'f'.repeat(16))]);
    column.setZoom(1000);
    expect(column.scale).toBe(20);
    column.setZoom(0.0001);
    expect(column.scale).toBe(0.1);
  });
});
