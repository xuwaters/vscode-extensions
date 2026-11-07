// @vitest-environment happy-dom
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  MAX_ZOOM,
  MIN_ZOOM,
  clampZoom,
  installZoom,
  stepZoom,
  wheelZoom,
  type PageZoom,
} from './zoom';

describe('zoom arithmetic', () => {
  it('steps along the ladder, from anywhere on or off it', () => {
    expect(stepZoom(1, 1)).toBe(1.25);
    expect(stepZoom(1, -1)).toBe(0.8);
    expect(stepZoom(1.37, 1)).toBe(1.5);
    expect(stepZoom(1.37, -1)).toBe(1.25);
  });

  it('stops at the ends of the range', () => {
    expect(stepZoom(MAX_ZOOM, 1)).toBe(MAX_ZOOM);
    expect(stepZoom(MIN_ZOOM, -1)).toBe(MIN_ZOOM);
    expect(wheelZoom(MAX_ZOOM, -100)).toBe(MAX_ZOOM);
    expect(wheelZoom(MIN_ZOOM, 100)).toBe(MIN_ZOOM);
  });

  it('caps a mouse notch at about one step and lets a pinch follow the fingers', () => {
    expect(wheelZoom(1, -100)).toBeCloseTo(1.25);
    expect(wheelZoom(1, 100)).toBeCloseTo(0.8);
    expect(wheelZoom(1, -2)).toBeCloseTo(Math.exp(0.02));
  });

  it('falls back to 100% for a zoom it cannot read', () => {
    expect(clampZoom(Number.NaN)).toBe(1);
    expect(clampZoom(0)).toBe(1);
    expect(clampZoom(100)).toBe(MAX_ZOOM);
  });
});

describe('installZoom', () => {
  let content: HTMLElement;
  let frontmatter: HTMLElement;
  let block: HTMLElement;
  let zoom: PageZoom;
  let suspended = false;
  const onChange = vi.fn();
  const onSettle = vi.fn();
  const scrollBy = vi.fn();

  function setScrollY(value: number): void {
    Object.defineProperty(window, 'scrollY', { configurable: true, value });
  }

  /** happy-dom has no layout, so give the block fixed metrics. */
  function placeBlock(top: number, height: number): void {
    block.getBoundingClientRect = () =>
      ({ top, height, bottom: top + height }) as DOMRect;
  }

  function wheel(init: WheelEventInit): WheelEvent {
    const event = new WheelEvent('wheel', { cancelable: true, ...init });
    // happy-dom's WheelEvent drops the modifier keys and the pointer from its init.
    for (const k of ['ctrlKey', 'shiftKey', 'altKey', 'metaKey'] as const) {
      Object.defineProperty(event, k, { value: init[k] ?? false });
    }
    for (const k of ['clientX', 'clientY'] as const) {
      Object.defineProperty(event, k, { value: init[k] ?? 0 });
    }
    window.dispatchEvent(event);
    return event;
  }

  const scale = () => Number(content.style.getPropertyValue('zoom'));

  // Once for the file: an install leaves its wheel listener on the window.
  beforeAll(() => {
    document.body.innerHTML =
      '<div id="frontmatter"></div><div id="content"><p id="block" data-sourcepos="1:1-1:5">text</p></div>';
    content = document.getElementById('content') as HTMLElement;
    frontmatter = document.getElementById('frontmatter') as HTMLElement;
    block = document.getElementById('block') as HTMLElement;
    window.scrollBy = scrollBy as unknown as typeof window.scrollBy;
    zoom = installZoom({
      scaled: [frontmatter, content],
      content,
      initial: 1,
      suspended: () => suspended,
      onChange,
      onSettle,
    });
  });

  beforeEach(() => {
    vi.useFakeTimers();
    suspended = false;
    setScrollY(0);
    placeBlock(100, 200);
    zoom.run('reset');
    vi.runAllTimers();
    vi.clearAllMocks();
    vi.spyOn(document, 'elementFromPoint').mockReturnValue(block);
  });

  afterEach(() => {
    vi.restoreAllMocks();
    vi.useRealTimers();
  });

  it('scales the document and the frontmatter alike', () => {
    zoom.run('in');
    expect(scale()).toBe(1.25);
    expect(frontmatter.style.getPropertyValue('zoom')).toBe('1.25');
    zoom.run('out');
    expect(scale()).toBe(1);
    zoom.run('out');
    expect(scale()).toBe(0.8);
    zoom.run('reset');
    expect(scale()).toBe(1);
  });

  it('zooms on ctrl+wheel and alt+wheel, taking the scroll from the browser', () => {
    expect(wheel({ deltaY: -100, ctrlKey: true }).defaultPrevented).toBe(true);
    expect(scale()).toBeCloseTo(1.25);
    expect(wheel({ deltaY: 100, altKey: true }).defaultPrevented).toBe(true);
    expect(scale()).toBeCloseTo(1);
  });

  it('reads a wheel that reports lines rather than pixels', () => {
    wheel({ deltaY: -1, deltaMode: 1, ctrlKey: true });
    expect(scale()).toBeCloseTo(Math.exp(0.16));
  });

  it('leaves a plain wheel alone', () => {
    expect(wheel({ deltaY: 100 }).defaultPrevented).toBe(false);
    expect(wheel({ deltaY: 100, shiftKey: true }).defaultPrevented).toBe(false);
    expect(scale()).toBe(1);
  });

  it('keeps out of the wheel while the image viewer is up', () => {
    suspended = true;
    expect(wheel({ deltaY: -100, altKey: true }).defaultPrevented).toBe(false);
    expect(scale()).toBe(1);
  });

  it('does not answer the keys itself, which VS Code would answer too', () => {
    const event = new KeyboardEvent('keydown', {
      key: '=',
      metaKey: true,
      bubbles: true,
      cancelable: true,
    });
    document.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(false);
    expect(scale()).toBe(1);
  });

  it('holds the spot under the pointer across a reflow', () => {
    // The pointer is a quarter of the way down the block...
    placeBlock(100, 200);
    const rects = vi.spyOn(block, 'getBoundingClientRect');
    rects.mockReturnValueOnce({ top: 100, height: 200 } as DOMRect);
    // ...which the zoom pushes down the page and rewraps taller.
    rects.mockReturnValueOnce({ top: 500, height: 400 } as DOMRect);

    wheel({ deltaY: -100, ctrlKey: true, clientX: 10, clientY: 150 });

    expect(scrollBy).toHaveBeenCalledWith({ top: 450, behavior: 'instant' });
  });

  it('holds a spot in the margin at its scaled distance from the block', () => {
    // No block under the pointer: it sits 50px above the only one there is.
    vi.spyOn(document, 'elementFromPoint').mockReturnValue(content);
    const rects = vi.spyOn(block, 'getBoundingClientRect');
    rects.mockReturnValueOnce({ top: 100, height: 200, bottom: 300 } as DOMRect);
    rects.mockReturnValueOnce({ top: 100, height: 200, bottom: 300 } as DOMRect);
    rects.mockReturnValueOnce({ top: 200, height: 400, bottom: 600 } as DOMRect);

    wheel({ deltaY: -100, ctrlKey: true, clientY: 50 });

    // 62.5px above the block now, which has moved to 200: the spot is at 137.5,
    // and it belongs back at 50.
    expect(scrollBy.mock.calls[0][0].top).toBeCloseTo(87.5);
  });

  it('holds the top of the view on a key step, but never leaves the top of the page', () => {
    zoom.run('in');
    expect(scrollBy).not.toHaveBeenCalled();

    setScrollY(400);
    zoom.run('in');
    expect(scrollBy).toHaveBeenCalledOnce();
  });

  it('reports the layout at once and the zoom only when it comes to rest', () => {
    wheel({ deltaY: -50, ctrlKey: true });
    wheel({ deltaY: -50, ctrlKey: true });
    expect(onChange).toHaveBeenCalledTimes(2);
    expect(onSettle).not.toHaveBeenCalled();

    vi.runAllTimers();
    expect(onSettle).toHaveBeenCalledOnce();
    expect(onSettle).toHaveBeenCalledWith(scale());
  });

  it('does nothing at the end of the range', () => {
    for (let i = 0; i < 20; i += 1) zoom.run('in');
    vi.runAllTimers();
    vi.clearAllMocks();
    zoom.run('in');
    expect(onChange).not.toHaveBeenCalled();
    vi.runAllTimers();
    expect(onSettle).not.toHaveBeenCalled();
  });
});
