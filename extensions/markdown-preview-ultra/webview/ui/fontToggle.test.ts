// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { FontToggle } from './fontToggle';

describe('FontToggle', () => {
  let toolbar: HTMLElement;

  beforeEach(() => {
    document.body.innerHTML = '<div id="bar"></div>';
    toolbar = document.getElementById('bar') as HTMLElement;
  });

  function button(): HTMLButtonElement {
    return document.getElementById('font-toggle') as HTMLButtonElement;
  }

  it('labels itself in the font a click switches to', () => {
    const toggle = new FontToggle(toolbar, () => {});

    toggle.update('proportional', false);
    expect(button().textContent).toBe('Aa');
    expect(button().classList.contains('to-mono')).toBe(true);
    expect(button().classList.contains('to-prose')).toBe(false);
    expect(button().title).toContain('editor font');

    toggle.update('monospace', false);
    expect(button().classList.contains('to-prose')).toBe(true);
    expect(button().classList.contains('to-mono')).toBe(false);
    expect(button().title).toContain('reading font');
  });

  it('marks itself while the configured font is overridden', () => {
    const toggle = new FontToggle(toolbar, () => {});

    toggle.update('monospace', true);
    expect(button().classList.contains('overridden')).toBe(true);

    toggle.update('monospace', false);
    expect(button().classList.contains('overridden')).toBe(false);
  });

  it('reports clicks and does not submit any enclosing form', () => {
    const onToggle = vi.fn();
    new FontToggle(toolbar, onToggle);

    expect(button().type).toBe('button');
    button().click();
    expect(onToggle).toHaveBeenCalledTimes(1);
  });
});
