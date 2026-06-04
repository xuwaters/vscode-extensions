// Mermaid is large, so it's dynamically imported (a separate webview chunk)
// and only loaded the first time a document actually contains a diagram.
type MermaidModule = typeof import('mermaid');

let mermaidModule: MermaidModule | null = null;
let mermaidLoading: Promise<MermaidModule> | null = null;

// Rendered SVG cache keyed by `${theme}::${source}` so unchanged diagrams
// aren't re-rendered on every keystroke (or when toggling the color theme).
const svgCache = new Map<string, string>();
let idSeq = 0;

async function loadMermaid(): Promise<MermaidModule> {
  if (mermaidModule) return mermaidModule;
  if (!mermaidLoading) {
    mermaidLoading = import('mermaid').then((mod) => {
      mermaidModule = mod;
      return mod;
    });
  }
  return mermaidLoading;
}

/** Render every `.mermaid-container` found inside `root`. */
export async function renderMermaid(
  root: HTMLElement,
  theme: 'light' | 'dark',
): Promise<void> {
  const containers = root.querySelectorAll<HTMLElement>('.mermaid-container');
  if (containers.length === 0) return;

  const mod = await loadMermaid();
  mod.default.initialize({
    startOnLoad: false,
    theme: theme === 'dark' ? 'dark' : 'default',
    securityLevel: 'strict',
  });

  for (const el of containers) {
    const source = decodeURIComponent(
      el.getAttribute('data-mermaid-source') ?? '',
    );
    if (!source) continue;

    const key = `${theme}::${source}`;
    const cached = svgCache.get(key);
    if (cached) {
      el.innerHTML = cached;
      continue;
    }

    el.innerHTML = '<div class="mermaid-loading">Rendering diagram…</div>';
    try {
      const { svg } = await mod.default.render(`mermaid-svg-${idSeq++}`, source);
      svgCache.set(key, svg);
      el.innerHTML = svg;
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      el.innerHTML = `<pre class="mermaid-error">${escapeHtml(message)}</pre>`;
    }
  }
}

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;');
}
