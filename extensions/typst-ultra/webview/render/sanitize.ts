/**
 * Turning what the compiler sent into something that can be put in the page.
 *
 * The two formats a page arrives in — SVG markup and base64 PNG — are checked
 * here and nowhere else, so there is a single door into the DOM.
 */

/**
 * Parse page SVG and strip anything that should not be there.
 *
 * The compiler emits a fixed vocabulary and cannot be made to emit `<script>`,
 * so this should always be a no-op. It is here because "should always" is not
 * "does", and surviving a compiler bug is better than executing it. The CSP
 * already makes an injected script unrunnable; this is the second layer.
 *
 * Parsing rather than concatenating also means a malformed document yields
 * nothing rather than a half-built DOM.
 */
export function adopt(svg: string): Element | null {
  const parsed = new DOMParser().parseFromString(svg, 'image/svg+xml');
  if (parsed.getElementsByTagName('parsererror').length > 0) return null;

  // Two bits of tolerance, both for the same reason: DOM implementations differ
  // on how an XML document exposes its root. Browsers populate
  // `documentElement`; some others only populate `firstElementChild`. And the
  // check is by tag name rather than `instanceof SVGElement`, because which
  // constructor the root is built from also varies — what matters is that it
  // says `<svg>`.
  const root = parsed.documentElement ?? parsed.firstElementChild;
  if (!root || root.tagName.toLowerCase() !== 'svg') return null;

  for (const element of Array.from(root.querySelectorAll('script, foreignObject'))) {
    element.remove();
  }
  for (const element of Array.from(root.querySelectorAll('*'))) {
    for (const attribute of Array.from(element.attributes)) {
      const name = attribute.name.toLowerCase();
      if (name.startsWith('on') || (name === 'href' && isScriptUrl(attribute.value))) {
        element.removeAttribute(attribute.name);
      }
    }
  }

  // The page element already carries the size; let the SVG fill it.
  root.setAttribute('width', '100%');
  root.setAttribute('height', '100%');
  return root;
}

function isScriptUrl(value: string): boolean {
  return /^\s*(javascript|data:text\/html|vbscript)/i.test(value);
}

/**
 * Wrap a base64 PNG as an `<img>`.
 *
 * The `data:` URI is what the CSP's `img-src` permits, and a raster page cannot
 * execute anything — which is why PNG mode needs none of the stripping SVG does.
 * The trade is what decision 0006 names: no zoom fidelity beyond the rendered
 * resolution, and no find-in-preview, because there is no text.
 */
export function rasterPage(base64: string): Element | null {
  if (!/^[A-Za-z0-9+/]+={0,2}$/.test(base64)) return null;

  const image = document.createElement('img');
  image.className = 'page-raster';
  image.src = `data:image/png;base64,${base64}`;
  image.decoding = 'async';
  return image;
}
