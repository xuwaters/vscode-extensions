// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { installEditButton } from './editButton';

describe('installEditButton', () => {
  let toolbar: HTMLElement;

  beforeEach(() => {
    document.body.innerHTML = '<div id="bar"></div>';
    toolbar = document.getElementById('bar') as HTMLElement;
  });

  function button(): HTMLButtonElement {
    return document.getElementById('edit-source') as HTMLButtonElement;
  }

  it('sits in the toolbar with a label saying where a click leads', () => {
    installEditButton(toolbar, () => {});

    expect(toolbar.contains(button())).toBe(true);
    expect(button().title).toContain('Edit');
  });

  it('reports clicks and does not submit any enclosing form', () => {
    const onEdit = vi.fn();
    installEditButton(toolbar, onEdit);

    expect(button().type).toBe('button');
    button().click();
    expect(onEdit).toHaveBeenCalledTimes(1);
  });
});
