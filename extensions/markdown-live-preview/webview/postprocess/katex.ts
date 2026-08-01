import katex from 'katex';

// Rendered-HTML cache keyed by `style::tex` so unchanged formulas inside a
// replaced block still skip typesetting.
const cache = new Map<string, string>();

/** Typeset every math span the engine marked, within `root`s. */
export function renderMath(roots: Element[]): void {
  for (const root of roots) {
    const spans = elementsIn(root, '[data-math-style]');
    for (const el of spans) {
      const tex = el.textContent ?? '';
      const display = el.getAttribute('data-math-style') === 'display';
      const key = `${display ? 'display' : 'inline'}::${tex}`;
      let html = cache.get(key);
      if (html === undefined) {
        try {
          html = katex.renderToString(tex, {
            displayMode: display,
            throwOnError: false,
            output: 'htmlAndMathml',
          });
        } catch (err) {
          html = `<span class="math-error">${escapeHtml(String(err))}</span>`;
        }
        cache.set(key, html);
      }
      el.innerHTML = html;
      el.classList.add('math-rendered');
    }
  }
}

/** `root` itself plus matching descendants. */
function elementsIn(root: Element, selector: string): Element[] {
  const out = root.matches(selector) ? [root] : [];
  out.push(...root.querySelectorAll(selector));
  return out;
}

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;');
}
