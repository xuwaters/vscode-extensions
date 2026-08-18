// @vitest-environment happy-dom
import { describe, expect, it } from 'vitest';
import { adopt, rasterPage } from './sanitize.js';

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

describe('wrapping a raster page', () => {
  it('builds a data: image from base64', () => {
    const image = rasterPage('iVBORw0KGgo=') as HTMLImageElement | null;
    expect(image?.getAttribute('src')).toBe('data:image/png;base64,iVBORw0KGgo=');
  });

  // The `src` is interpolated into a URI, so anything that is not base64 has no
  // business being there — a `"` or a newline would end the data URI early.
  it('refuses anything that is not base64', () => {
    expect(rasterPage('not base64!')).toBeNull();
    expect(rasterPage('iVBO"onerror=')).toBeNull();
    expect(rasterPage('')).toBeNull();
  });
});
