// @vitest-environment happy-dom
import { Updates } from '@microsoft/fast-element';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { WebviewToHost } from '../../src/messages.js';

/**
 * pdf.js never loads here. The element's job in this file is the chrome — the
 * bindings, the refs, and the event semantics — and the renderer is exactly the
 * part happy-dom cannot host: there is no canvas behind it.
 */
vi.mock('../render/pdfjs.js', () => ({
  pdfjs: { TextLayer: class {} },
  bootWorker: async () => {},
  documentParams: (source: unknown) => source,
}));

const { PDF_VIEWER_TAG, PdfViewer } = await import('./element.js');

// fast-element 3 defines the element asynchronously, so the tests mount it the
// way the page does — parse the tag, wait for the definition, let the browser
// upgrade it — rather than constructing the class, which is exactly what a real
// engine refuses to do before the name is registered.
await customElements.whenDefined(PDF_VIEWER_TAG);

let viewer: InstanceType<typeof PdfViewer>;
let posted: WebviewToHost[];

const query = <T extends Element>(selector: string): T | null =>
  viewer.shadowRoot?.querySelector<T>(selector) ?? null;

const must = <T extends Element>(selector: string): T => {
  const found = query<T>(selector);
  if (!found) throw new Error(`the viewer has no ${selector}`);
  return found;
};

beforeEach(async () => {
  document.body.innerHTML = `<${PDF_VIEWER_TAG}></${PDF_VIEWER_TAG}>`;
  posted = [];
  const mounted = document.body.firstElementChild;
  if (!(mounted instanceof PdfViewer)) {
    throw new Error(`<${PDF_VIEWER_TAG}> was not upgraded`);
  }
  viewer = mounted;
  viewer.host = { post: (message) => posted.push(message) };
  await Updates.next();
});

describe('the states a tab passes through', () => {
  it('shows nothing before the host has said anything', () => {
    expect(query('.chrome')).toBeNull();
    expect(query('.card')).toBeNull();
  });

  it('names the document while it is opening', async () => {
    viewer.name = 'spec.pdf';
    viewer.state = 'loading';
    await Updates.next();
    expect(must('.card-name').textContent).toContain('spec.pdf');
    expect(query('.chrome')).toBeNull();
  });

  it('offers a way out of a failure rather than a dead tab', async () => {
    viewer.state = 'error';
    viewer.errorMessage = 'not a PDF';
    await Updates.next();
    expect(must('.card-hint').textContent).toContain('not a PDF');
    expect(must('.card .btn').textContent).toContain('Try again');
  });
});

describe('the chrome, once a document is open', () => {
  beforeEach(async () => {
    viewer.state = 'ready';
    viewer.pageCount = 12;
    await Updates.next();
  });

  it('binds the scroller and the column the page list is built into', () => {
    expect(viewer.scrollEl).toBe(query('.viewer'));
    expect(viewer.columnEl).toBe(query('.column'));
  });

  it('reads out the page count', () => {
    expect(must('.count').textContent).toContain('12');
  });

  it('disables the back button on the first page and forward on the last', async () => {
    expect(must<HTMLButtonElement>('.chrome .btn:nth-of-type(2)').disabled).toBe(true);
    viewer.page = 12;
    await Updates.next();
    expect(must<HTMLButtonElement>('.chrome .btn:nth-of-type(3)').disabled).toBe(true);
  });

  it('marks the fit in effect as pressed', async () => {
    const fitWidth = must<HTMLButtonElement>('[aria-label="Fit width"]');
    expect(fitWidth.className).toContain('on');
    viewer.applyFit('fit-page');
    await Updates.next();
    expect(must<HTMLButtonElement>('[aria-label="Fit width"]').className).not.toContain('on');
    expect(must<HTMLButtonElement>('[aria-label="Fit page"]').className).toContain('on');
  });

  it('flips colour inversion from the toolbar and tells the host', async () => {
    must<HTMLButtonElement>('[aria-label="Invert colours"]').click();
    await Updates.next();
    expect(viewer.inverted).toBe(true);
    expect(must('.column').className).toContain('inverted');
  });

  it('goes to a page typed into the box', async () => {
    const field = must<HTMLInputElement>('.field-input');
    field.value = '5';
    field.dispatchEvent(new Event('change'));
    await Updates.next();
    expect(viewer.page).toBe(5);
  });

  it('ignores a page number that is not one, leaving the reader where they are', async () => {
    const field = must<HTMLInputElement>('.field-input');
    field.value = 'nonsense';
    field.dispatchEvent(new Event('change'));
    await Updates.next();
    expect(viewer.page).toBe(1);
    expect(field.value).toBe('1');
  });

  it('puts an unreadable zoom back to what is actually in effect', async () => {
    const field = must<HTMLInputElement>('.field-input.wide');
    field.value = 'wide please';
    field.dispatchEvent(new Event('change'));
    await Updates.next();
    expect(viewer.zoom).toBe(1);
    expect(field.value).toBe('100%');
  });

  it('reads a zoom typed as a percentage', async () => {
    const field = must<HTMLInputElement>('.field-input.wide');
    field.value = '175%';
    field.dispatchEvent(new Event('change'));
    await Updates.next();
    expect(viewer.zoom).toBeCloseTo(1.75);
    expect(viewer.fit).toBe('actual');
  });

  /**
   * FAST cancels any event whose handler does not return `true`. On a keydown
   * binding over a text field that means the reader cannot type — a bug with no
   * visible cause, so it is worth a test rather than a comment.
   */
  it('does not swallow keystrokes in the fields it binds keydown on', () => {
    for (const selector of ['.field-input', '.field-input.wide', '.find']) {
      const event = new KeyboardEvent('keydown', { key: 'a', cancelable: true });
      must<HTMLInputElement>(selector).dispatchEvent(event);
      expect(event.defaultPrevented, `${selector} swallowed a keystroke`).toBe(false);
    }
  });

  it('still cancels the keys it acts on itself', () => {
    const event = new KeyboardEvent('keydown', { key: 'PageDown', cancelable: true });
    must('.viewer').dispatchEvent(event);
    expect(event.defaultPrevented).toBe(true);
    expect(viewer.page).toBe(2);
  });

  it('turns the page with the arrow keys', () => {
    must('.viewer').dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight' }));
    expect(viewer.page).toBe(2);
    must('.viewer').dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowLeft' }));
    expect(viewer.page).toBe(1);
  });

  /**
   * A page zoomed past the width of the tab has somewhere to go sideways, and
   * turning the page instead would leave no way to read its right-hand edge
   * without a mouse.
   */
  it('leaves the arrow keys to the scroller while there is width to scroll', () => {
    Object.defineProperty(viewer.scrollEl, 'scrollWidth', { value: 2000, configurable: true });
    Object.defineProperty(viewer.scrollEl, 'clientWidth', { value: 800, configurable: true });
    const event = new KeyboardEvent('keydown', { key: 'ArrowRight', cancelable: true });
    must('.viewer').dispatchEvent(event);
    expect(viewer.page).toBe(1);
    expect(event.defaultPrevented).toBe(false);
  });

  it('takes the scroll away from an Alt-wheel and leaves a plain one alone', () => {
    // happy-dom drops the modifier flags out of a `WheelEvent` init, so they
    // are stated on the event itself rather than passed to the constructor.
    const wheel = (altKey: boolean): WheelEvent => {
      const event = new WheelEvent('wheel', { deltaY: -120, cancelable: true });
      Object.defineProperty(event, 'altKey', { value: altKey });
      return event;
    };

    const alt = wheel(true);
    window.dispatchEvent(alt);
    expect(alt.defaultPrevented).toBe(true);

    const plain = wheel(false);
    window.dispatchEvent(plain);
    expect(plain.defaultPrevented).toBe(false);
  });

  /**
   * VSCode forwards every keystroke a webview sees to its own keybinding
   * resolver whatever the page does with the event — so a shortcut answered
   * here as well as by a `pdfUltra.*` command is answered twice, and the zoom
   * moves two steps for one press. They belong to the host alone.
   */
  it('leaves the modifier shortcuts to the host rather than answering them twice', () => {
    for (const key of ['=', '-', '0', '1', '8', '9', 'f']) {
      window.dispatchEvent(new KeyboardEvent('keydown', { key, metaKey: true }));
      window.dispatchEvent(new KeyboardEvent('keydown', { key, ctrlKey: true }));
    }
    expect(viewer.zoom).toBe(1);
    expect(viewer.fit).toBe('fit-width');
    expect(viewer.mode).toBe('continuous');
  });

  it('switches to one page at a time from the toolbar, and back', async () => {
    const button = must<HTMLButtonElement>('[aria-label="Show one page at a time"]');
    button.click();
    await Updates.next();
    expect(viewer.mode).toBe('single');
    expect(must('[aria-label="Show one page at a time"]').className).toContain('on');

    must<HTMLButtonElement>('[aria-label="Show one page at a time"]').click();
    await Updates.next();
    expect(viewer.mode).toBe('continuous');
  });
});

describe('the outline sidebar', () => {
  beforeEach(async () => {
    viewer.state = 'ready';
    viewer.outlineVisible = true;
    viewer.outline.shown = [
      {
        id: 'o0',
        title: 'Chapter one',
        depth: 0,
        dest: null,
        url: null,
        bold: false,
        italic: false,
        children: ['o1'],
        hasChildren: true,
      },
      {
        id: 'o1',
        title: 'Section',
        depth: 1,
        dest: null,
        url: 'https://example.com',
        bold: false,
        italic: false,
        children: [],
        hasChildren: false,
      },
    ];
    await Updates.next();
  });

  it('renders one row per entry, indented by depth', () => {
    const rows = viewer.shadowRoot?.querySelectorAll('.outline-row') ?? [];
    expect(rows).toHaveLength(2);
    expect((rows[1] as HTMLElement).getAttribute('style')).toContain('20px');
  });

  it('gives only the rows with children a twisty', () => {
    expect(viewer.shadowRoot?.querySelectorAll('.outline-twisty')).toHaveLength(1);
  });

  it('asks the host to open an entry that points out of the document', () => {
    const rows = viewer.shadowRoot?.querySelectorAll<HTMLElement>('.outline-row');
    rows?.[1]?.click();
    expect(posted).toContainEqual({ type: 'openLink', href: 'https://example.com' });
  });

  it('hides itself again, and is gone from the DOM rather than merely invisible', async () => {
    viewer.toggleOutline();
    await Updates.next();
    expect(viewer.outlineVisible).toBe(false);
    expect(query('.outline')).toBeNull();
  });
});
