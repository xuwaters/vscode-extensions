// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { NavButtons } from './nav';

describe('NavButtons', () => {
  let toolbar: HTMLElement;

  beforeEach(() => {
    document.body.innerHTML = '<div id="bar"></div>';
    toolbar = document.getElementById('bar') as HTMLElement;
  });

  function buttons(): HTMLButtonElement[] {
    return Array.from(toolbar.querySelectorAll('button'));
  }

  it('stays hidden until there is somewhere to go', () => {
    const nav = new NavButtons(toolbar, () => {});
    expect(buttons().every((b) => b.hidden)).toBe(true);

    nav.update(true, false);
    expect(buttons().every((b) => b.hidden)).toBe(false);
  });

  it('disables the direction that has no history', () => {
    const nav = new NavButtons(toolbar, () => {});
    nav.update(true, false);

    const [back, forward] = buttons();
    expect(back.disabled).toBe(false);
    expect(forward.disabled).toBe(true);
  });

  it('reports the direction of a click', () => {
    const onNavigate = vi.fn();
    const nav = new NavButtons(toolbar, onNavigate);
    nav.update(true, true);

    const [back, forward] = buttons();
    back.click();
    forward.click();

    expect(onNavigate.mock.calls).toEqual([['back'], ['forward']]);
  });
});
