let mermaidModule: typeof import('mermaid') | null = null;
let mermaidLoading: Promise<typeof import('mermaid')> | null = null;

/** Lazy-load the mermaid library. */
async function loadMermaid(): Promise<typeof import('mermaid')> {
  if (mermaidModule) return mermaidModule;
  if (mermaidLoading) return mermaidLoading;

  mermaidLoading = import('mermaid').then((mod) => {
    mermaidModule = mod;
    return mod;
  });

  return mermaidLoading;
}

/** Render a mermaid diagram from source code, returning an SVG string. */
export async function renderMermaidDiagram(
  source: string,
  theme: 'light' | 'dark' = 'light',
): Promise<string> {
  const mod = await loadMermaid();
  mod.default.initialize({
    startOnLoad: false,
    theme: theme === 'dark' ? 'dark' : 'default',
  });

  const id = `mermaid-${Math.random().toString(36).slice(2, 9)}`;
  try {
    const { svg } = await mod.default.render(id, source);
    return svg;
  } catch (err) {
    const message = err instanceof Error ? err.message : 'Unknown error';
    return `<pre class="mermaid-error">Mermaid error: ${escapeHtml(message)}</pre>`;
  }
}

/** Initialize all mermaid containers within a root element. */
export async function initMermaidContainers(
  root: HTMLElement,
  theme: 'light' | 'dark' = 'light',
): Promise<void> {
  const containers = root.querySelectorAll<HTMLElement>('.mermaid-container');
  if (containers.length === 0) return;

  for (const el of containers) {
    const source = decodeURIComponent(
      el.getAttribute('data-mermaid-source') ?? '',
    );
    if (!source) continue;

    el.innerHTML = '<div class="mermaid-loading">Rendering diagram...</div>';
    const svg = await renderMermaidDiagram(source, theme);
    el.innerHTML = svg;
  }
}

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;');
}
