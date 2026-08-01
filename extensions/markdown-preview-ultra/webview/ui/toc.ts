import type { TocEntry } from '../../src/messages';

export const TOC_DEFAULT_WIDTH = 240;
const TOC_MIN_WIDTH = 140;
const TOC_MAX_WIDTH = 720;
/** Page space always left for the document, however far the sash is dragged. */
const CONTENT_RESERVE = 120;

/** Keep the sidebar between its bounds and inside the current viewport. */
export function clampTocWidth(width: number, viewport: number): number {
  const max = Math.max(
    TOC_MIN_WIDTH,
    Math.min(TOC_MAX_WIDTH, viewport - CONTENT_RESERVE),
  );
  if (!Number.isFinite(width)) return TOC_DEFAULT_WIDTH;
  return Math.round(Math.min(Math.max(width, TOC_MIN_WIDTH), max));
}

/**
 * Collapsible TOC sidebar plus per-heading hover anchors. Dependency-free:
 * a fixed `<aside>` with a toggle button in the floating toolbar; the active
 * heading is tracked while scrolling. Its left edge is a drag sash — long
 * headings are otherwise ellipsised at the default width.
 */
export class TocSidebar {
  private readonly aside: HTMLElement;
  private readonly list: HTMLElement;
  private readonly toggle: HTMLButtonElement;
  private readonly sash: HTMLElement;
  private visible = false;
  private slugs: string[] = [];
  /** The width the user asked for; the applied one is this, clamped. */
  private requestedWidth = TOC_DEFAULT_WIDTH;

  constructor(
    toolbar: HTMLElement,
    private readonly onNavigate: (entry: { slug: string; line: number }) => void,
    private readonly onVisibilityChange: (visible: boolean) => void,
    private readonly onWidthChange: (width: number) => void,
  ) {
    this.toggle = document.createElement('button');
    this.toggle.id = 'toc-toggle';
    this.toggle.type = 'button';
    this.toggle.title = 'Toggle table of contents';
    this.toggle.textContent = '☰';
    this.toggle.addEventListener('click', () => this.setVisible(!this.visible));

    this.aside = document.createElement('aside');
    this.aside.id = 'toc-sidebar';
    this.sash = document.createElement('div');
    this.sash.className = 'toc-sash';
    this.sash.title = 'Drag to resize (double-click to reset)';
    const body = document.createElement('div');
    body.className = 'toc-body';
    const heading = document.createElement('div');
    heading.className = 'toc-heading';
    heading.textContent = 'Contents';
    this.list = document.createElement('div');
    this.list.className = 'toc-list';
    body.append(heading, this.list);
    this.aside.append(this.sash, body);

    toolbar.append(this.toggle);
    document.body.append(this.aside);

    this.sash.addEventListener('pointerdown', (e) => this.startResize(e));
    this.sash.addEventListener('dblclick', () =>
      this.setWidth(TOC_DEFAULT_WIDTH, true),
    );

    document.addEventListener('scroll', () => this.highlightActive(), {
      passive: true,
    });
    // A narrower window may no longer fit the requested width.
    window.addEventListener('resize', () => this.setWidth(this.requestedWidth));
  }

  /** Apply a sidebar width, clamped to the viewport. */
  setWidth(width: number, persist = false): void {
    this.requestedWidth = Number.isFinite(width) ? width : TOC_DEFAULT_WIDTH;
    const applied = clampTocWidth(this.requestedWidth, window.innerWidth);
    document.documentElement.style.setProperty('--toc-width', `${applied}px`);
    if (persist) this.onWidthChange(applied);
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
        // Keep the click away from VSCode's own <body> link handler.
        e.stopPropagation();
        this.onNavigate(entry);
      });
      li.append(a);
      this.slugs.push(entry.slug);
      if (entry.children.length > 0) li.append(this.renderList(entry.children));
      ul.append(li);
    }
    return ul;
  }

  /** Drag the sash: the sidebar is right-anchored, so it grows leftward. */
  private startResize(event: PointerEvent): void {
    if (event.button !== 0) return;
    event.preventDefault();
    const startX = event.clientX;
    const startWidth = clampTocWidth(this.requestedWidth, window.innerWidth);
    this.sash.setPointerCapture(event.pointerId);
    this.aside.classList.add('resizing');
    document.body.classList.add('toc-resizing');

    const move = (e: PointerEvent): void =>
      this.setWidth(startWidth + (startX - e.clientX));
    const end = (): void => {
      this.sash.removeEventListener('pointermove', move);
      this.sash.removeEventListener('pointerup', end);
      this.sash.removeEventListener('pointercancel', end);
      this.aside.classList.remove('resizing');
      document.body.classList.remove('toc-resizing');
      this.setWidth(this.requestedWidth, true);
    };
    this.sash.addEventListener('pointermove', move);
    this.sash.addEventListener('pointerup', end);
    this.sash.addEventListener('pointercancel', end);
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
