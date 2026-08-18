// @vitest-environment happy-dom
import { describe, expect, it } from 'vitest';
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
