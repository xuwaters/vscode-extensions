/**
 * Floating light/dark switch for the rendered page.
 *
 * Flipping it overrides the configured `markdownPreviewUltra.theme` — the
 * setting is never written. The override is held by the host for the whole
 * window, so it carries across files and both preview surfaces, and it is
 * dropped as soon as the user changes the configured theme (see
 * `src/override.ts`).
 */
export class ThemeToggle {
  private readonly button: HTMLButtonElement;

  constructor(toolbar: HTMLElement, onToggle: () => void) {
    this.button = document.createElement('button');
    this.button.id = 'theme-toggle';
    this.button.type = 'button';
    this.button.addEventListener('click', () => onToggle());
    toolbar.append(this.button);
  }

  /**
   * Reflect the theme the page is currently showing. `overridden` marks the
   * button when the shown theme differs from the configured one.
   */
  update(kind: 'light' | 'dark', overridden: boolean): void {
    const target = kind === 'light' ? 'dark' : 'light';
    // Show the theme a click switches *to*.
    this.button.textContent = kind === 'light' ? '☾' : '☀';
    this.button.title = overridden
      ? `Switch to ${target} theme — overriding the configured theme for this preview`
      : `Switch to ${target} theme (this preview only)`;
    this.button.classList.toggle('overridden', overridden);
  }
}
