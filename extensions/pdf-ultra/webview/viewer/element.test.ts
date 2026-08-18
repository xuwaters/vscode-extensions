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

  it('disables the previous-page button on the first page and next on the last', async () => {
    expect(must<HTMLButtonElement>('[aria-label="Previous page"]').disabled).toBe(true);
    viewer.page = 12;
    await Updates.next();
    expect(must<HTMLButtonElement>('[aria-label="Next page"]').disabled).toBe(true);
  });

  it('offers nothing to go back to until something has jumped', () => {
    expect(must<HTMLButtonElement>('[aria-label="Go back"]').disabled).toBe(true);
  });

  /**
   * The page box is a jump — the reader named somewhere rather than scrolled
   * there — so it is a move the button can undo, and it comes back to the spot
   * they left rather than to the top of the page.
   */
  it('goes back to where a jump started, and then has nowhere left to go', async () => {
    viewer.page = 4;
    const field = must<HTMLInputElement>('.field-input');
    field.value = '9';
    field.dispatchEvent(new Event('change'));
    await Updates.next();
    expect(viewer.page).toBe(9);

    const back = must<HTMLButtonElement>('[aria-label="Go back"]');
    expect(back.disabled).toBe(false);
    back.click();
    await Updates.next();
    expect(viewer.page).toBe(4);
    expect(field.value).toBe('4');
    expect(must<HTMLButtonElement>('[aria-label="Go back"]').disabled).toBe(true);
  });

  it('leaves a page turn out of the history: that is what the arrows are', async () => {
    viewer.goToPage(5);
    await Updates.next();
    expect(must<HTMLButtonElement>('[aria-label="Go back"]').disabled).toBe(true);
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

  /**
   * Go to Page is the box that is already on screen, selected and ready to be
   * typed over — not a quick-pick over the top of the document asking for a
   * number the toolbar is already showing.
   */
  it('puts the reader in the page box when the host asks for a page', () => {
    viewer.handle({ type: 'command', command: 'focusPage' });
    expect(viewer.shadowRoot?.activeElement).toBe(must('.field-input'));
  });

  it('goes to a page typed into the box', async () => {
    const field = must<HTMLInputElement>('.field-input');
    field.value = '5';
    field.dispatchEvent(new Event('change'));
    await Updates.next();
    expect(viewer.page).toBe(5);
  });

  /**
   * Out of range names an end of the document rather than a mistake, so it
   * goes there — the alternative is a box that silently rejects what was typed
   * into it and puts the old number back.
   */
  it('clamps a page past either end of the document to that end', async () => {
    const field = must<HTMLInputElement>('.field-input');
    field.value = '0';
    field.dispatchEvent(new Event('change'));
    await Updates.next();
    expect(viewer.page).toBe(1);
    expect(field.value).toBe('1');

    field.value = '200';
    field.dispatchEvent(new Event('change'));
    await Updates.next();
    expect(viewer.page).toBe(12);
    expect(field.value).toBe('12');
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

/**
 * A toolbar of unlabelled glyphs is a puzzle until something names them, and
 * `title` is not the instrument: a second of hover before anything appears, an
 * OS tooltip that knows nothing of the editor's theme, and nowhere to put the
 * shortcut that does the same thing.
 */
describe('the toolbar explaining itself', () => {
  beforeEach(async () => {
    viewer.state = 'ready';
    viewer.pageCount = 12;
    await Updates.next();
  });

  /** Every control in the toolbar, whether it is a glyph or a box. */
  const controls = (): HTMLElement[] => [
    ...(viewer.shadowRoot?.querySelectorAll<HTMLElement>('.chrome .btn, .chrome input') ?? []),
  ];

  it('leaves no control in the toolbar unexplained', () => {
    expect(controls().length).toBeGreaterThan(10);
    for (const control of controls()) {
      expect(control.dataset.tip, control.getAttribute('aria-label') ?? '').toBeTruthy();
    }
  });

  /** The old instrument, in the one place it cannot be styled or hurried. */
  it('does not fall back on the browser’s own tooltip', () => {
    for (const control of controls()) expect(control.hasAttribute('title')).toBe(false);
  });

  it('names a button the moment it takes focus, with the key that does the same', async () => {
    must<HTMLElement>('[aria-label="Rotate clockwise"]').dispatchEvent(
      new FocusEvent('focusin', { bubbles: true }),
    );
    await Updates.next();
    expect(viewer.tip?.text).toBe('Rotate clockwise');
    expect(must('.tip').textContent).toContain('Rotate clockwise');

    must<HTMLElement>('[aria-label="Toggle outline"]').dispatchEvent(
      new FocusEvent('focusin', { bubbles: true }),
    );
    await Updates.next();
    expect(must('.tip-keys').textContent).toMatch(/K/);
  });

  /**
   * The wait is what keeps a tooltip out of the way of a reader who is only
   * passing over the toolbar; dropping it for the *second* control is what
   * makes a row of glyphs readable in one pass rather than one wait per icon.
   */
  it('waits for the first tip and then follows the pointer without waiting', () => {
    vi.useFakeTimers();
    try {
      const over = (label: string): void => {
        must<HTMLElement>(`[aria-label="${label}"]`).dispatchEvent(
          new Event('pointerover', { bubbles: true }),
        );
      };

      over('Fit width');
      expect(viewer.tip).toBeNull();
      vi.advanceTimersByTime(400);
      expect(viewer.tip?.text).toBe('Fit width');

      over('Fit page');
      expect(viewer.tip?.text).toBe('Fit page');
    } finally {
      vi.useRealTimers();
    }
  });

  it('stops explaining when the pointer leaves the toolbar', () => {
    must<HTMLElement>('[aria-label="Fit page"]').dispatchEvent(
      new FocusEvent('focusin', { bubbles: true }),
    );
    expect(viewer.tip).not.toBeNull();
    must('.chrome').dispatchEvent(new Event('pointerleave'));
    expect(viewer.tip).toBeNull();
  });

  /** A tip under the box the reader is typing in is in the way of the typing. */
  it('says nothing when a text box takes focus', () => {
    must<HTMLElement>('.find').dispatchEvent(new FocusEvent('focusin', { bubbles: true }));
    expect(viewer.tip).toBeNull();
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
