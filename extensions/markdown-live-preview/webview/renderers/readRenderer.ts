import type { DocumentModel, TextChange } from '../documentModel';
import type { Renderer } from './types';

export class ReadRenderer implements Renderer {
  private container: HTMLElement | null = null;
  private model: DocumentModel | null = null;

  mount(container: HTMLElement, model: DocumentModel): void {
    this.container = container;
    this.model = model;
    container.className = 'read-mode';
    this.renderFull();
  }

  teardown(): void {
    if (this.container) {
      this.container.innerHTML = '';
      this.container.className = '';
    }
    this.container = null;
    this.model = null;
  }

  onDocumentChanged(_changes: TextChange[]): void {
    // Re-render the full document on any change
    this.renderFull();
  }

  getCursorLine(): number | null {
    return null; // Read mode has no cursor
  }

  getScrollOffset(): number {
    return this.container?.scrollTop ?? 0;
  }

  setCursorLine(_line: number): void {
    // No cursor in read mode; could scroll to the line's block instead
  }

  setScrollOffset(offset: number): void {
    if (this.container) {
      this.container.scrollTop = offset;
    }
  }

  private renderFull(): void {
    if (!this.container || !this.model) return;
    const html = this.model.renderAll();
    this.container.innerHTML = `<div class="read-content">${html}</div>`;

    // Initialize mermaid diagrams
    this.initMermaid();
  }

  private async initMermaid(): Promise<void> {
    if (!this.container) return;
    const mermaidContainers = this.container.querySelectorAll('.mermaid-container');
    if (mermaidContainers.length === 0) return;

    try {
      const mermaid = await import('mermaid');
      mermaid.default.initialize({
        startOnLoad: false,
        theme: document.body.classList.contains('vscode-dark') ? 'dark' : 'default',
      });

      for (const el of mermaidContainers) {
        const source = decodeURIComponent(
          el.getAttribute('data-mermaid-source') ?? '',
        );
        if (!source) continue;

        const id = `mermaid-${Math.random().toString(36).slice(2, 9)}`;
        try {
          const { svg } = await mermaid.default.render(id, source);
          el.innerHTML = svg;
        } catch {
          el.innerHTML = `<pre class="mermaid-error">Mermaid rendering error</pre>`;
        }
      }
    } catch {
      // Mermaid not available; leave the raw pre blocks
    }
  }
}
