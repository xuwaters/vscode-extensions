// @vitest-environment happy-dom
import { describe, expect, it } from 'vitest';
import { PageList, adopt } from './pageList.js';

/**
 * The SVG the preview inserts is engine-generated: `typst_svg` emits a fixed
 * vocabulary from the layout frame, and a typst document has no way to inject
 * raw markup into the paged export target. So this stripping should always be a
 * no-op — which is exactly why it is here. If it ever is not, that is a compiler
 * bug, and surviving it beats executing it.
 */
describe('adopting page SVG', () => {
  it('parses a normal page', () => {
    const root = adopt(
      '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 595 842">' +
        '<g><path d="M0 0 L10 10" /><use href="#g1" x="1.5" y="2.5" /></g>' +
        '</svg>',
    );

    expect(root).not.toBeNull();
    expect(root!.querySelectorAll('path')).toHaveLength(1);
    expect(root!.querySelectorAll('use')).toHaveLength(1);
  });

  it('yields nothing rather than a partial DOM when there is no SVG root', () => {
    // Whether an unclosed tag is *repaired* or reported as a parse error is the
    // XML parser's business and differs between implementations; what `adopt`
    // guarantees is that a document without an `<svg>` root yields nothing at
    // all rather than something half-built.
    expect(adopt('not svg at all')).toBeNull();
    expect(adopt('')).toBeNull();
    expect(adopt('<html><body>hello</body></html>')).toBeNull();
  });

  it('strips a script element', () => {
    const root = adopt(
      '<svg xmlns="http://www.w3.org/2000/svg"><script>globalThis.pwned = 1</script><g /></svg>',
    );
    expect(root!.querySelectorAll('script')).toHaveLength(0);
  });

  it('strips foreignObject, which can carry arbitrary HTML', () => {
    const root = adopt(
      '<svg xmlns="http://www.w3.org/2000/svg"><foreignObject><div>x</div></foreignObject></svg>',
    );
    expect(root!.querySelectorAll('foreignObject')).toHaveLength(0);
  });

  it('strips every on* handler attribute', () => {
    const root = adopt(
      '<svg xmlns="http://www.w3.org/2000/svg">' +
        '<g onload="pwned()" onclick="pwned()" ONMOUSEOVER="pwned()" fill="red" />' +
        '</svg>',
    );

    const group = root!.querySelector('g')!;
    expect(group.getAttribute('onload')).toBeNull();
    expect(group.getAttribute('onclick')).toBeNull();
    expect(group.getAttribute('ONMOUSEOVER')).toBeNull();
    // Legitimate attributes survive.
    expect(group.getAttribute('fill')).toBe('red');
  });

  it('strips a javascript: href but keeps a real link', () => {
    const root = adopt(
      '<svg xmlns="http://www.w3.org/2000/svg">' +
        '<a href="javascript:alert(1)"><text>bad</text></a>' +
        '<a href="https://typst.app"><text>good</text></a>' +
        '</svg>',
    );

    const links = root!.querySelectorAll('a');
    expect(links[0].getAttribute('href')).toBeNull();
    expect(links[1].getAttribute('href')).toBe('https://typst.app');
  });

  it('lets the page element own the size', () => {
    const root = adopt(
      '<svg xmlns="http://www.w3.org/2000/svg" width="595pt" height="842pt" />',
    );
    expect(root!.getAttribute('width')).toBe('100%');
    expect(root!.getAttribute('height')).toBe('100%');
  });

  it('keeps embedded data: images, which the CSP permits and which cannot run', () => {
    const root = adopt(
      '<svg xmlns="http://www.w3.org/2000/svg">' +
        '<image href="data:image/png;base64,iVBORw0KGgo=" />' +
        '</svg>',
    );
    expect(root!.querySelector('image')!.getAttribute('href')).toMatch(/^data:image\/png/);
  });
});

/**
 * The panel follows the active editor, so one page list outlives several
 * documents. Handing it a different document has to leave nothing of the old
 * one behind — not its pages, not its cache, and not its scroll position.
 */
describe('changing the document under the page list', () => {
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
    list: PageList;
    container: HTMLElement;
    reports: Report[];
  } {
    const container = document.createElement('div');
    document.body.replaceChildren(container);
    const reports: Report[] = [];
    const list = new PageList(
      container,
      (first, last, known) => reports.push({ first, last, known }),
      () => undefined,
    );
    return { list, container, reports };
  }

  it('drops every page and returns to the top', () => {
    const { list, container } = build();
    list.setMetrics([metric(0, 'a'.repeat(16)), metric(1, 'b'.repeat(16))]);
    expect(list.length).toBe(2);
    container.scrollTop = 400;

    list.reset();

    expect(list.length).toBe(0);
    expect(container.children).toHaveLength(0);
    expect(container.scrollTop).toBe(0);
  });

  it('leaves nothing for the next document to inherit by hash', () => {
    const { list, container, reports } = build();
    const hash = 'c'.repeat(16);
    list.setMetrics([metric(0, hash)]);
    list.applyPatches([
      {
        op: 'replace',
        index: 0,
        hash,
        format: 'svg',
        content: '<svg xmlns="http://www.w3.org/2000/svg"><g /></svg>',
      },
    ]);
    expect(container.querySelectorAll('svg')).toHaveLength(1);

    list.reset();
    // A new document can carry the same hash — an empty first page, say. It
    // must be fetched again rather than served from the cache of the document
    // that has just been replaced.
    list.setMetrics([metric(0, hash)]);

    expect(container.querySelectorAll('svg')).toHaveLength(0);
    expect(reports.at(-1)?.known).toEqual({});
  });
});
