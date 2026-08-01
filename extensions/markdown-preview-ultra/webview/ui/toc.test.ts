// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { TOC_DEFAULT_WIDTH, TocSidebar, clampTocWidth } from './toc';

describe('clampTocWidth', () => {
  it('keeps a sane width untouched', () => {
    expect(clampTocWidth(320, 1400)).toBe(320);
  });

  it('holds the bounds', () => {
    expect(clampTocWidth(20, 1400)).toBe(140);
    expect(clampTocWidth(5000, 4000)).toBe(720);
  });

  it('always leaves room for the document', () => {
    expect(clampTocWidth(400, 400)).toBe(280);
  });

  it('prefers the minimum over a viewport too small for either', () => {
    expect(clampTocWidth(200, 100)).toBe(140);
  });

  it('falls back to the default for a broken width', () => {
    expect(clampTocWidth(NaN, 1400)).toBe(TOC_DEFAULT_WIDTH);
  });
});

describe('TocSidebar width', () => {
  let toolbar: HTMLElement;

  beforeEach(() => {
    document.body.innerHTML = '<div id="bar"></div>';
    document.documentElement.style.removeProperty('--toc-width');
    toolbar = document.getElementById('bar') as HTMLElement;
    window.innerWidth = 1400;
  });

  function make(onWidthChange = vi.fn()): {
    toc: TocSidebar;
    onWidthChange: ReturnType<typeof vi.fn>;
  } {
    const toc = new TocSidebar(
      toolbar,
      () => {},
      () => {},
      onWidthChange,
    );
    return { toc, onWidthChange };
  }

  function appliedWidth(): string {
    return document.documentElement.style.getPropertyValue('--toc-width');
  }

  it('publishes the width as a CSS variable', () => {
    const { toc } = make();
    toc.setWidth(360);
    expect(appliedWidth()).toBe('360px');
  });

  it('persists only when asked', () => {
    const { toc, onWidthChange } = make();

    toc.setWidth(360);
    expect(onWidthChange).not.toHaveBeenCalled();

    toc.setWidth(360, true);
    expect(onWidthChange).toHaveBeenCalledWith(360);
  });

  it('persists the clamped width, not the requested one', () => {
    const { toc, onWidthChange } = make();
    toc.setWidth(5000, true);
    expect(onWidthChange).toHaveBeenCalledWith(720);
  });

  it('restores the requested width when the window grows again', () => {
    const { toc } = make();
    toc.setWidth(600);

    window.innerWidth = 500;
    window.dispatchEvent(new Event('resize'));
    expect(appliedWidth()).toBe('380px');

    window.innerWidth = 1400;
    window.dispatchEvent(new Event('resize'));
    expect(appliedWidth()).toBe('600px');
  });

  it('resets to the default on a double-click of the sash', () => {
    const { toc, onWidthChange } = make();
    toc.setWidth(600);

    const sash = document.querySelector('.toc-sash') as HTMLElement;
    sash.dispatchEvent(new Event('dblclick'));

    expect(appliedWidth()).toBe(`${TOC_DEFAULT_WIDTH}px`);
    expect(onWidthChange).toHaveBeenCalledWith(TOC_DEFAULT_WIDTH);
  });
});
