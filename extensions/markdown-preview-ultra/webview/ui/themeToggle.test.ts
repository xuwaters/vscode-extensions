// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { ThemeToggle, reconcileOverride } from './themeToggle';

describe('reconcileOverride', () => {
  it('keeps an override while its configured theme is unchanged', () => {
    const stored = { override: 'github-dark', base: 'github-light' } as const;
    expect(reconcileOverride(stored, 'github-light')).toEqual(stored);
  });

  it('drops the override when the configured theme changes', () => {
    expect(
      reconcileOverride(
        { override: 'github-dark', base: 'github-light' },
        'auto',
      ),
    ).toEqual({});
  });

  it('adopts the configured theme as the base when none was recorded', () => {
    expect(reconcileOverride({ override: 'github-dark' }, 'auto')).toEqual({
      override: 'github-dark',
      base: 'auto',
    });
  });

  it('is a no-op without an override', () => {
    expect(reconcileOverride({}, 'github-light')).toEqual({});
    expect(reconcileOverride({ base: 'github-light' }, 'auto')).toEqual({});
  });
});

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
