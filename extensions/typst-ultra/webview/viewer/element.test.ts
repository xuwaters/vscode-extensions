// @vitest-environment happy-dom
import { Updates } from '@microsoft/fast-element';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type {
  PreviewPlace,
  PreviewSettings,
  WebviewToHost,
} from '../../src/preview/messages.js';
import { PX_PER_PT } from '../model/layout.js';

const SETTINGS: PreviewSettings = {
  scrollSync: 'both',
  cursorIndicator: true,
  invertColors: 'never',
  background: 'editor',
  renderMode: 'svg',
};

const { TYPST_PREVIEW_TAG, TypstPreview } = await import('./element.js');

// fast-element 3 defines the element asynchronously, so the tests mount it the
// way the page does — parse the tag, wait for the definition, let the browser
// upgrade it — rather than constructing the class, which is exactly what a real
// engine refuses to do before the name is registered.
await customElements.whenDefined(TYPST_PREVIEW_TAG);

let preview: InstanceType<typeof TypstPreview>;
let posted: WebviewToHost[];
let saved: PreviewPlace[];

const A4 = { widthPt: 595, heightPt: 842 };

const query = <T extends Element>(selector: string): T | null =>
  preview.querySelector<T>(selector);

const must = <T extends Element>(selector: string): T => {
  const found = query<T>(selector);
  if (!found) throw new Error(`the preview has no ${selector}`);
  return found;
};

const button = (label: string): HTMLButtonElement =>
  must<HTMLButtonElement>(`.btn[aria-label="${label}"]`);

/**
 * happy-dom lays nothing out, so the scroller's content box is whatever we say
 * it is. That box is the whole input to a fit, which is what these tests are
 * about.
 */
function sizeViewport(w: number, h: number): void {
  Object.defineProperty(preview.scrollEl, 'clientWidth', { value: w, configurable: true });
  Object.defineProperty(preview.scrollEl, 'clientHeight', { value: h, configurable: true });
}

/** The viewport width at which one A4 page fills it exactly, padding included. */
const widthFor = (zoom: number): number => A4.widthPt * PX_PER_PT * zoom + 32;

function openDocument(pages = 1): void {
  preview.handle({
    type: 'metrics',
    seq: 1,
    uri: 'file:///doc.typ',
    pages: Array.from({ length: pages }, (_, index) => ({
      index,
      ...A4,
      hash: String(index).repeat(16).slice(0, 16),
    })),
  });
}

beforeEach(async () => {
  document.body.innerHTML = `<${TYPST_PREVIEW_TAG}></${TYPST_PREVIEW_TAG}>`;
  posted = [];
  saved = [];
  const mounted = document.body.firstElementChild;
  if (!(mounted instanceof TypstPreview)) {
    throw new Error(`<${TYPST_PREVIEW_TAG}> was not upgraded`);
  }
  preview = mounted;
  preview.host = {
    post: (message) => posted.push(message),
    save: (place) => saved.push(place),
  };
  await Updates.next();
});

describe('the chrome', () => {
  it('renders a toolbar and a page column', () => {
    expect(query('.chrome')).not.toBeNull();
    expect(query('.pages')).not.toBeNull();
  });

  it('shows nothing about the compile while it is going well', async () => {
    preview.handle({ type: 'status', state: 'ok' });
    await Updates.next();
    expect(query('.status')).toBeNull();
  });

  it('keeps the last good pages on screen, dimmed, when a compile fails', async () => {
    vi.useFakeTimers();
    try {
      preview.handle({ type: 'status', state: 'error', message: 'unexpected }' });
      vi.advanceTimersByTime(1000);
    } finally {
      vi.useRealTimers();
    }
    await Updates.next();
    expect(must('.status').textContent).toContain('unexpected }');
    expect(must('.pages').classList.contains('has-error')).toBe(true);
  });

  /**
   * Typing passes through half-written expressions, so `error` and `compiling`
   * arrive and leave several times a second. Announcing each of them would put
   * a bar on screen and dim every page, twice per keystroke.
   */
  it('says nothing about a compile state that does not last', async () => {
    vi.useFakeTimers();
    try {
      preview.handle({ type: 'status', state: 'compiling' });
      vi.advanceTimersByTime(100);
      preview.handle({ type: 'status', state: 'error', message: 'unexpected }' });
      vi.advanceTimersByTime(100);
      preview.handle({ type: 'status', state: 'ok' });
      vi.advanceTimersByTime(1000);
    } finally {
      vi.useRealTimers();
    }
    await Updates.next();
    expect(query('.status')).toBeNull();
    expect(must('.pages').classList.contains('has-error')).toBe(false);
  });

  it('counts the pages', async () => {
    openDocument(3);
    await Updates.next();
    expect(must('.count').textContent).toContain('3');
  });
});

/**
 * The point of the whole exercise: a fit is a mode, not a one-shot. It stays
 * switched on, it says so, and every change of the panel's size re-resolves it.
 */
describe('a fit stays switched on', () => {
  beforeEach(() => {
    openDocument();
    sizeViewport(widthFor(1), 400);
  });

  it('resolves the zoom and lights the button', async () => {
    preview.applyFit('width');
    await Updates.next();

    expect(preview.zoom).toBeCloseTo(1, 6);
    expect(button('Fit width').classList.contains('on')).toBe(true);
    expect(button('Fit width').getAttribute('aria-pressed')).toBe('true');
    expect(button('Fit page').classList.contains('on')).toBe(false);
  });

  it('follows the panel as it is resized', () => {
    preview.applyFit('width');
    expect(preview.zoom).toBeCloseTo(1, 6);

    sizeViewport(widthFor(2), 400);
    preview.revalidate();

    expect(preview.zoom).toBeCloseTo(2, 6);
    expect(preview.fit).toBe('width');
  });

  it('fits the whole page inside the panel, not just its width', () => {
    preview.applyFit('page');
    // The box is a page wide but nothing like a page tall, so the height is
    // what binds.
    expect(preview.zoom).toBeLessThan(1);
    expect(preview.zoom).toBeCloseTo((400 - 32) / (A4.heightPt * PX_PER_PT), 6);
  });

  /**
   * VSCode keeps a hidden webview alive and lays it out at nothing. Resolving a
   * fit against a 0×0 box lands at the bottom of the zoom range — which is how
   * a document comes back from a background tab at 10%.
   */
  it('leaves the zoom alone while the panel has no size', () => {
    preview.applyFit('width');
    sizeViewport(0, 0);
    preview.revalidate();
    expect(preview.zoom).toBeCloseTo(1, 6);
  });

  it('survives a recompile, at the size the panel is now', () => {
    preview.applyFit('width');
    sizeViewport(widthFor(1.5), 400);

    preview.handle({ type: 'metrics', seq: 2, uri: 'file:///doc.typ', pages: [{ index: 0, ...A4, hash: 'a'.repeat(16) }] });

    expect(preview.fit).toBe('width');
    expect(preview.zoom).toBeCloseTo(1.5, 6);
  });
});

describe('switching a fit off', () => {
  beforeEach(() => {
    openDocument();
    sizeViewport(widthFor(1), 400);
    preview.applyFit('width');
  });

  it('is what a zoom step does', async () => {
    preview.zoomBy(1);
    await Updates.next();

    expect(preview.fit).toBe('actual');
    expect(preview.zoom).toBeGreaterThan(1);
    expect(button('Fit width').classList.contains('on')).toBe(false);
  });

  it('is what typing a zoom does', () => {
    const box = must<HTMLInputElement>('.field-input.wide');
    box.value = '175%';
    box.dispatchEvent(new Event('change'));

    expect(preview.fit).toBe('actual');
    expect(preview.zoom).toBeCloseTo(1.75, 6);
  });

  it('leaves the zoom alone when what was typed is not one', () => {
    const box = must<HTMLInputElement>('.field-input.wide');
    box.value = 'wide please';
    box.dispatchEvent(new Event('change'));

    expect(preview.zoom).toBeCloseTo(1, 6);
    expect(box.value).toBe('100%');
  });

  it('holds the zoom where it was once the fit is off', () => {
    preview.setZoom(1.25);
    sizeViewport(widthFor(3), 400);
    preview.revalidate();

    expect(preview.zoom).toBeCloseTo(1.25, 6);
  });
});

describe('remembering how the preview was set up', () => {
  it('adopts what the host remembers', () => {
    preview.handle({
      type: 'init',
      settings: SETTINGS,
      restore: { zoom: 1.4, fit: 'page', inverted: true },
    });

    expect(preview.fit).toBe('page');
    expect(preview.inverted).toBe(true);
  });

  it("prefers the panel's own memory to the host's", () => {
    preview.restore({ zoom: 2, fit: 'actual', inverted: false }, { override: true });
    preview.handle({
      type: 'init',
      settings: SETTINGS,
      restore: { zoom: 1, fit: 'width', inverted: true },
    });

    expect(preview.fit).toBe('actual');
    expect(preview.zoom).toBeCloseTo(2, 6);
    expect(preview.inverted).toBe(false);
  });

  /**
   * The settings that arrive with `init` are the first this element has heard,
   * not a change to what it had — so they must not read as one and undo the
   * inversion the reader had switched off.
   */
  it('does not let the settings that arrive with it overrule what it restored', () => {
    preview.restore({ zoom: 1, fit: 'width', inverted: false }, { override: true });
    preview.handle({
      type: 'init',
      settings: { ...SETTINGS, invertColors: 'always' },
    });

    expect(preview.inverted).toBe(false);
  });

  it("leaves the reader's inversion alone when some other setting changes", () => {
    preview.handle({ type: 'init', settings: SETTINGS });
    preview.toggleInvert();

    preview.handle({ type: 'settings', settings: { ...SETTINGS, background: 'white' } });

    expect(preview.inverted).toBe(true);
  });

  // Changing the setting is how the reader takes their own toggle back.
  it('does follow the setting itself changing', () => {
    preview.handle({ type: 'init', settings: SETTINGS });
    preview.toggleInvert();
    expect(preview.inverted).toBe(true);

    // `auto` against a light theme, which is what the test page is.
    preview.handle({ type: 'settings', settings: { ...SETTINGS, invertColors: 'auto' } });

    expect(preview.inverted).toBe(false);
  });

  it('tells the host when the reader changes it', async () => {
    openDocument();
    sizeViewport(widthFor(1), 400);
    preview.applyFit('page');
    await new Promise((resolve) => setTimeout(resolve, 250));

    expect(saved.at(-1)?.fit).toBe('page');
  });
});

/**
 * The column is the only thing in the page that scrolls, and scrolling from the
 * keyboard goes to whatever holds the focus — so a preview whose focus is on
 * nothing answers to no arrow at all.
 */
describe('the focus', () => {
  it('is on the page column from the moment the preview is mounted', () => {
    expect(document.activeElement).toBe(preview.scrollEl);
  });

  it('goes back to the column when the host says the tab is active', () => {
    preview.scrollEl.blur();
    expect(document.activeElement).not.toBe(preview.scrollEl);

    preview.handle({ type: 'focus' });

    expect(document.activeElement).toBe(preview.scrollEl);
  });

  it('is left where the reader put it', () => {
    const box = must<HTMLInputElement>('.field-input');
    box.focus();

    preview.handle({ type: 'focus' });

    expect(document.activeElement).toBe(box);
  });
});

describe('turning the page', () => {
  const press = (key: string): KeyboardEvent => {
    const event = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true });
    must('.pages').dispatchEvent(event);
    return event;
  };

  beforeEach(() => {
    openDocument(3);
    sizeViewport(widthFor(1), 400);
  });

  it('is what the arrow keys do over the column', () => {
    expect(press('ArrowRight').defaultPrevented).toBe(true);
    expect(preview.pageField).toBe('2');

    press('ArrowRight');
    expect(preview.pageField).toBe('3');

    press('ArrowLeft');
    expect(preview.pageField).toBe('2');
  });

  /** The set `pdf-ultra` answers to, so both viewers are read the same way. */
  it('answers to PgUp, PgDn, Home and End as well', () => {
    press('PageDown');
    expect(preview.pageField).toBe('2');

    press('PageUp');
    expect(preview.pageField).toBe('1');

    press('End');
    expect(preview.pageField).toBe('3');

    press('Home');
    expect(preview.pageField).toBe('1');
  });

  it('stops at both ends rather than running off them', () => {
    press('ArrowLeft');
    expect(preview.pageField).toBe('1');

    preview.goToPage(2);
    press('ArrowRight');
    expect(preview.pageField).toBe('3');
  });

  /**
   * A page zoomed past the width of the tab has somewhere to go sideways, and
   * turning the page instead would leave no way to read its right-hand edge
   * without a mouse.
   */
  it('leaves the arrows to the scroller while there is width to scroll', () => {
    Object.defineProperty(preview.scrollEl, 'scrollWidth', {
      value: 2000,
      configurable: true,
    });

    expect(press('ArrowRight').defaultPrevented).toBe(false);
    expect(preview.pageField).toBe('1');

    // PgDn has no sideways meaning to give up, so it still turns the page.
    press('PageDown');
    expect(preview.pageField).toBe('2');
  });

  /** Ctrl+→ and friends are the editor's word-wise keys, not ours. */
  it('leaves a modified arrow alone', () => {
    const event = new KeyboardEvent('keydown', {
      key: 'ArrowRight',
      ctrlKey: true,
      bubbles: true,
      cancelable: true,
    });
    must('.pages').dispatchEvent(event);

    expect(preview.pageField).toBe('1');
  });

  /**
   * FAST cancels any event whose handler does not return `true`. On a keydown
   * binding that means the reader cannot type — a bug with no visible cause, so
   * it is worth a test rather than a comment.
   */
  it('does not swallow the keystrokes it does not act on', () => {
    for (const selector of ['.pages', '.field-input', '.field-input.wide']) {
      const event = new KeyboardEvent('keydown', { key: 'a', cancelable: true });
      must(selector).dispatchEvent(event);
      expect(event.defaultPrevented, `${selector} swallowed a keystroke`).toBe(false);
    }
  });

  /**
   * The bug this set of tests grew out of: a tab reached with Cmd+Shift+] is
   * handed the focus with nothing in the page holding it, and keys bound to the
   * column never arrived — so the page could only be turned after a click.
   */
  it('turns the page with the focus on nothing in particular', () => {
    const event = new KeyboardEvent('keydown', {
      key: 'ArrowRight',
      bubbles: true,
      cancelable: true,
    });
    document.body.dispatchEvent(event);

    expect(preview.pageField).toBe('2');
  });

  /** The caret in the page box is what an arrow moves there, not the page. */
  it('leaves the arrows to the toolbar boxes', () => {
    const box = must<HTMLInputElement>('.field-input');
    box.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true, cancelable: true }),
    );

    expect(preview.pageField).toBe('1');
  });
});

describe('talking to the extension host', () => {
  it('asks for the pages that came into view', () => {
    openDocument(2);
    expect(posted.some((message) => message.type === 'viewport')).toBe(true);
  });

  it('hands a link to the host rather than navigating', () => {
    const anchor = document.createElement('a');
    anchor.setAttribute('href', 'https://typst.app');
    must('.pages').append(anchor);

    anchor.dispatchEvent(new Event('click', { bubbles: true, cancelable: true }));

    expect(posted).toContainEqual({ type: 'openLink', href: 'https://typst.app' });
  });

  it('leaves an internal anchor to the document itself', () => {
    const anchor = document.createElement('a');
    anchor.setAttribute('href', '#section-2');
    must('.pages').append(anchor);

    anchor.dispatchEvent(new Event('click', { bubbles: true, cancelable: true }));

    expect(posted.some((message) => message.type === 'openLink')).toBe(false);
  });

  it('sends the reader back to the source, and out to an export', () => {
    button('Edit source').click();
    button('Export').click();

    expect(posted).toContainEqual({ type: 'openSource' });
    expect(posted).toContainEqual({ type: 'export' });
  });

  /** The host has no other way to reach a toggle that lives only in the page. */
  it('reads a negative page as the invert command', () => {
    expect(preview.inverted).toBe(false);
    preview.handle({ type: 'goToPage', page: -1 });
    expect(preview.inverted).toBe(true);
  });
});
