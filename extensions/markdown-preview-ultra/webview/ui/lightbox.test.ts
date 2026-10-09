// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { installLightbox, zoomInLevel, zoomOutLevel } from './lightbox';

describe('zoom levels', () => {
  it('steps through VS Code image-preview levels', () => {
    expect(zoomInLevel(1)).toBe(1.5);
    expect(zoomInLevel(0.55)).toBe(0.6);
    expect(zoomOutLevel(1)).toBe(0.9);
    expect(zoomOutLevel(2.5)).toBe(2);
  });

  it('stops at the ends', () => {
    expect(zoomInLevel(20)).toBe(20);
    expect(zoomOutLevel(0.1)).toBe(0.1);
  });
});

describe('installLightbox', () => {
  let content: HTMLElement;

  beforeEach(() => {
    document.body.innerHTML = '<main id="content"><img id="pic" src="a.png"><a href="#"><img id="linked" src="b.png"></a></main>';
    content = document.getElementById('content') as HTMLElement;
    installLightbox(content);
  });

  // Each install leaves window listeners behind; close it so it goes quiet.
  afterEach(() => {
    key({ key: 'Escape' });
  });

  const overlay = () => document.getElementById('lightbox') as HTMLElement;
  const viewer = () => overlay().querySelector('.lightbox-scroller img') as HTMLImageElement;
  const label = () => overlay().querySelector('.lightbox-zoom')?.textContent;

  /** happy-dom has no layout, so give the viewer image fixed metrics. */
  function sizeViewer(natural: number, shown: number): void {
    Object.defineProperty(viewer(), 'naturalWidth', { configurable: true, value: natural });
    Object.defineProperty(viewer(), 'clientWidth', { configurable: true, value: shown });
  }

  function openPic(): void {
    document.getElementById('pic')?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
  }

  function key(init: KeyboardEventInit): KeyboardEvent {
    const event = new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init });
    document.dispatchEvent(event);
    return event;
  }

  it('opens scaled to fit on an image click, but not for linked images', () => {
    document.getElementById('linked')?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    expect(overlay().classList.contains('visible')).toBe(false);

    openPic();
    expect(overlay().classList.contains('visible')).toBe(true);
    expect(viewer().getAttribute('src')).toContain('a.png');
    expect(viewer().classList.contains('fit')).toBe(true);
  });

  it('zooms in a step from the fitted size when the image is clicked', () => {
    openPic();
    sizeViewer(400, 200);
    viewer().dispatchEvent(new MouseEvent('click', { bubbles: true }));

    expect(viewer().classList.contains('fit')).toBe(false);
    expect(viewer().style.width).toBe('240px');
    expect(viewer().style.height).toBe('');
    expect(label()).toBe('60%');
  });

  it('zooms with ctrl/cmd + and -, returns to fit on 0, and keeps the keys from the page', () => {
    openPic();
    sizeViewer(100, 100);
    const pageZoom = vi.fn();
    document.addEventListener('keydown', pageZoom);

    expect(key({ key: '=', ctrlKey: true }).defaultPrevented).toBe(true);
    expect(label()).toBe('150%');
    key({ key: '=', metaKey: true });
    expect(label()).toBe('200%');
    expect(viewer().classList.contains('pixelated')).toBe(false);
    key({ key: '=', ctrlKey: true });
    expect(viewer().classList.contains('pixelated')).toBe(true);
    key({ key: '-', ctrlKey: true });
    expect(label()).toBe('200%');
    key({ key: '0', ctrlKey: true });
    expect(viewer().classList.contains('fit')).toBe(true);
    expect(pageZoom).not.toHaveBeenCalled();

    document.removeEventListener('keydown', pageZoom);
  });

  function wheel(init: WheelEventInit): WheelEvent {
    const event = new WheelEvent('wheel', { bubbles: true, cancelable: true, ...init });
    // happy-dom's WheelEvent drops the modifier keys from its init.
    for (const k of ['ctrlKey', 'shiftKey', 'altKey', 'metaKey'] as const) {
      Object.defineProperty(event, k, { value: init[k] ?? false });
    }
    viewer().dispatchEvent(event);
    return event;
  }

  it('zooms a fixed step per ctrl+wheel event, however small its delta', () => {
    openPic();
    sizeViewer(1000, 500);

    expect(wheel({ deltaY: -1, ctrlKey: true }).defaultPrevented).toBe(true);
    expect(viewer().style.width).toBe('537.5px');
    for (let i = 0; i < 40; i++) wheel({ deltaY: -2, ctrlKey: true });
    expect(Number.parseFloat(viewer().style.width)).toBeGreaterThan(5000);
    wheel({ deltaY: 3, ctrlKey: true });
    expect(label()).toBe(`${Math.round(0.5 * 1.075 ** 41 * 0.925 * 100)}%`);
  });

  it('scrolls on a plain wheel and sideways on shift+wheel', () => {
    openPic();
    sizeViewer(100, 100);

    expect(wheel({ deltaY: 50 }).defaultPrevented).toBe(false);
    expect(wheel({ deltaY: 50, shiftKey: true }).defaultPrevented).toBe(true);
    expect(wheel({ deltaX: 50, shiftKey: true }).defaultPrevented).toBe(false);
  });

  it('leaves the page zoom keys alone while closed', () => {
    const event = key({ key: '=', ctrlKey: true });
    expect(event.defaultPrevented).toBe(false);
  });

  it('closes on escape, the close button, or a click beside the image', () => {
    openPic();
    key({ key: 'Escape' });
    expect(overlay().classList.contains('visible')).toBe(false);
    expect(viewer().hasAttribute('src')).toBe(false);

    openPic();
    (overlay().querySelector('.lightbox-close') as HTMLElement).click();
    expect(overlay().classList.contains('visible')).toBe(false);

    openPic();
    overlay().querySelector('.lightbox-scroller')?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    expect(overlay().classList.contains('visible')).toBe(false);
  });
});
