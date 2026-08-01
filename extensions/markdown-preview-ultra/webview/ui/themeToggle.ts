import type { PreviewTheme } from '../../src/messages';

/** A light/dark override plus the configured theme it was made against. */
export interface ThemeOverride {
  override?: PreviewTheme;
  base?: PreviewTheme;
}

/**
 * An override holds only while the theme it was made against is still the
 * configured one: changing `markdownPreviewUltra.theme` is an explicit choice
 * and retires the switch (that is also the way back to `auto`). An override
 * with no base was made before the first settings message arrived, so it
 * adopts whatever the configuration turns out to be.
 */
export function reconcileOverride(
  stored: ThemeOverride,
  configured: PreviewTheme,
): ThemeOverride {
  if (stored.override === undefined) return {};
  if (stored.base === undefined) {
    return { override: stored.override, base: configured };
  }
  return stored.base === configured ? stored : {};
}

/**
 * Floating light/dark switch for the rendered page.
 *
 * Flipping it overrides the configured `markdownPreviewUltra.theme` for this
 * preview only — the setting is never written. The override lives in webview
 * state (so it survives a panel reload) and is dropped as soon as the user
 * changes the configured theme.
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
