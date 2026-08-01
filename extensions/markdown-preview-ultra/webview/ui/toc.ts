import type { TocEntry } from '../../src/messages';

/**
 * Collapsible TOC sidebar plus per-heading hover anchors. Dependency-free:
 * a fixed `<aside>` with a toggle button in the floating toolbar; the active
 * heading is tracked while scrolling.
 */
export class TocSidebar {
  private readonly aside: HTMLElement;
  private readonly list: HTMLElement;
  private readonly toggle: HTMLButtonElement;
  private visible = false;
  private slugs: string[] = [];

  constructor(
    toolbar: HTMLElement,
    private readonly onNavigate: (entry: { slug: string; line: number }) => void,
    private readonly onVisibilityChange: (visible: boolean) => void,
  ) {
    this.toggle = document.createElement('button');
    this.toggle.id = 'toc-toggle';
    this.toggle.type = 'button';
    this.toggle.title = 'Toggle table of contents';
    this.toggle.textContent = '☰';
    this.toggle.addEventListener('click', () => this.setVisible(!this.visible));

    this.aside = document.createElement('aside');
    this.aside.id = 'toc-sidebar';
    const heading = document.createElement('div');
    heading.className = 'toc-heading';
    heading.textContent = 'Contents';
    this.list = document.createElement('div');
    this.list.className = 'toc-list';
    this.aside.append(heading, this.list);

    toolbar.append(this.toggle);
    document.body.append(this.aside);

    document.addEventListener('scroll', () => this.highlightActive(), {
      passive: true,
    });
  }

  setVisible(visible: boolean, persist = true): void {
    this.visible = visible;
    this.aside.classList.toggle('visible', visible);
    document.body.classList.toggle('toc-open', visible);
    if (persist) this.onVisibilityChange(visible);
    if (visible) this.highlightActive();
  }

  update(toc: TocEntry[]): void {
    this.list.textContent = '';
    this.slugs = [];
    if (toc.length > 0) this.list.append(this.renderList(toc));
    this.toggle.style.display = toc.length > 0 ? '' : 'none';
    if (toc.length === 0) this.setVisible(false, false);
    this.highlightActive();
  }

  private renderList(entries: TocEntry[]): HTMLElement {
    const ul = document.createElement('ul');
    for (const entry of entries) {
      const li = document.createElement('li');
      const a = document.createElement('a');
      a.href = `#${entry.slug}`;
      a.textContent = entry.text;
      a.dataset.slug = entry.slug;
      a.addEventListener('click', (e) => {
        e.preventDefault();
        this.onNavigate(entry);
      });
      li.append(a);
      this.slugs.push(entry.slug);
      if (entry.children.length > 0) li.append(this.renderList(entry.children));
      ul.append(li);
    }
    return ul;
  }

  /** Mark the last heading at/above the viewport top as active. */
  private highlightActive(): void {
    if (!this.visible || this.slugs.length === 0) return;
    let active: string | null = null;
    for (const slug of this.slugs) {
      const el = document.getElementById(slug);
      if (!el) continue;
      if (el.getBoundingClientRect().top <= 40) active = slug;
    }
    for (const a of this.list.querySelectorAll<HTMLElement>('a[data-slug]')) {
      a.classList.toggle('active', a.dataset.slug === active);
    }
  }
}

/** Add hover `¶` anchors to headings inside the changed blocks. */
export function addHeadingAnchors(roots: Element[]): void {
  for (const root of roots) {
    const selector = 'h1[id], h2[id], h3[id], h4[id], h5[id], h6[id]';
    const headings: Element[] = root.matches(selector) ? [root] : [];
    headings.push(...root.querySelectorAll(selector));
    for (const h of headings) {
      if (h.querySelector(':scope > .heading-anchor')) continue;
      const a = document.createElement('a');
      a.className = 'heading-anchor';
      a.href = `#${h.id}`;
      a.textContent = '¶';
      a.title = 'Copy link to heading';
      h.append(a);
    }
  }
}
