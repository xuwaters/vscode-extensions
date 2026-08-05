// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { ThemeToggle } from './themeToggle';

describe('ThemeToggle', () => {
  let toolbar: HTMLElement;

  beforeEach(() => {
    document.body.innerHTML = '<div id="bar"></div>';
    toolbar = document.getElementById('bar') as HTMLElement;
  });

  function button(): HTMLButtonElement {
    return document.getElementById('theme-toggle') as HTMLButtonElement;
  }

  it('shows the theme a click switches to', () => {
    const toggle = new ThemeToggle(toolbar, () => {});

    toggle.update('light', false);
    expect(button().textContent).toBe('☾');
    expect(button().title).toContain('dark');

    toggle.update('dark', false);
    expect(button().textContent).toBe('☀');
    expect(button().title).toContain('light');
  });

  it('marks itself while the configured theme is overridden', () => {
    const toggle = new ThemeToggle(toolbar, () => {});

    toggle.update('dark', true);
    expect(button().classList.contains('overridden')).toBe(true);

    toggle.update('dark', false);
    expect(button().classList.contains('overridden')).toBe(false);
  });

  it('reports clicks and does not submit any enclosing form', () => {
    const onToggle = vi.fn();
    new ThemeToggle(toolbar, onToggle);

    expect(button().type).toBe('button');
    button().click();
    expect(onToggle).toHaveBeenCalledTimes(1);
  });
});
