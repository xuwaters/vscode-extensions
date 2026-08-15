/**
 * Floating prose/monospace switch for the rendered page.
 *
 * Flipping it overrides the configured `markdownPreviewUltra.font` — the
 * setting is never written. Like the light/dark switch beside it, the override
 * is held by the host for the whole window, so it carries across files and both
 * preview surfaces, and it is dropped as soon as the user changes the setting
 * (see `src/override.ts`).
 */
import type { PreviewFont } from '../../src/messages';

export class FontToggle {
  private readonly button: HTMLButtonElement;

  constructor(toolbar: HTMLElement, onToggle: () => void) {
    this.button = document.createElement('button');
    this.button.id = 'font-toggle';
    this.button.type = 'button';
    this.button.textContent = 'Aa';
    this.button.addEventListener('click', () => onToggle());
    toolbar.append(this.button);
  }

  /**
   * Reflect the font the page is currently set in. `overridden` marks the
   * button when that differs from the configured font.
   */
  update(font: PreviewFont, overridden: boolean): void {
    const mono = font === 'monospace';
    // The label is set in the font a click switches *to*, so the button shows
    // what it does rather than describing it.
    this.button.classList.toggle('to-mono', !mono);
    this.button.classList.toggle('to-prose', mono);
    const target = mono ? 'the reading font' : 'the editor font';
    this.button.title = overridden
      ? `Switch to ${target} — overriding the configured font for this preview`
      : `Switch to ${target} (this preview only)`;
    this.button.classList.toggle('overridden', overridden);
  }
}
