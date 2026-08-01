// Mermaid is large, so it's dynamically imported (a separate webview chunk)
// and only loaded the first time a document actually contains a diagram.
type MermaidModule = typeof import('mermaid');

export type ResolvedMermaidTheme = 'default' | 'dark' | 'forest' | 'neutral';

let mermaidModule: MermaidModule | null = null;
let mermaidLoading: Promise<MermaidModule> | null = null;
let initializedTheme: ResolvedMermaidTheme | null = null;

// Rendered SVG cache keyed by `${theme}::${source}` so unchanged diagrams
// aren't re-rendered on every keystroke (or when toggling the color theme).
const svgCache = new Map<string, string>();
let idSeq = 0;

/** One pathological diagram must not wedge the preview. */
const RENDER_TIMEOUT_MS = 10_000;

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

/** Render every `.mermaid-container` found inside `roots`. */
export async function renderMermaid(
  roots: Element[],
  theme: ResolvedMermaidTheme,
): Promise<void> {
  const containers: HTMLElement[] = [];
  for (const root of roots) {
    if (root.matches('.mermaid-container')) {
      containers.push(root as HTMLElement);
    }
    containers.push(
      ...root.querySelectorAll<HTMLElement>('.mermaid-container'),
    );
  }
  if (containers.length === 0) return;

  const mod = await loadMermaid();
  if (initializedTheme !== theme) {
    mod.default.initialize({
      startOnLoad: false,
      theme,
      securityLevel: 'strict',
    });
    initializedTheme = theme;
  }

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
      const { svg } = await withTimeout(
        mod.default.render(`mermaid-svg-${idSeq++}`, source),
        RENDER_TIMEOUT_MS,
      );
      svgCache.set(key, svg);
      el.innerHTML = svg;
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      el.innerHTML = `<pre class="mermaid-error">${escapeHtml(message)}</pre>`;
    }
  }
}

function withTimeout<T>(promise: Promise<T>, ms: number): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(
      () => reject(new Error(`mermaid render timed out after ${ms / 1000}s`)),
      ms,
    );
    promise.then(
      (v) => {
        clearTimeout(timer);
        resolve(v);
      },
      (e) => {
        clearTimeout(timer);
        reject(e);
      },
    );
  });
}

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;');
}
