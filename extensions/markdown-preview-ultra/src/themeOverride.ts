/**
 * The rule behind the preview's in-page light/dark switch.
 *
 * Kept free of the `vscode` module so the rule itself is testable; the state it
 * is applied to lives in `themeStore.ts`.
 *
 * The switch is a deviation from `markdownPreviewUltra.theme`, never a write to
 * it. It holds only while the theme it was made against is still the configured
 * one: changing the setting is an explicit choice and retires the switch (that
 * is also the way back to `auto`).
 */
import type { PreviewTheme } from './messages';

/** A light/dark override plus the configured theme it was made against. */
export interface ThemeOverrideState {
  override?: PreviewTheme;
  base?: PreviewTheme;
}

/** Drop an override whose configured theme has changed under it. */
export function reconcileOverride(
  stored: ThemeOverrideState,
  configured: PreviewTheme,
): ThemeOverrideState {
  if (stored.override === undefined) return {};
  return stored.base === configured ? stored : {};
}

/**
 * The state left behind by flipping the switch to `chosen`. Landing back on the
 * configured theme retires the override entirely, rather than recording an
 * override that happens to agree with it.
 */
export function chooseOverride(
  chosen: PreviewTheme,
  configured: PreviewTheme,
): ThemeOverrideState {
  return chosen === configured ? {} : { override: chosen, base: configured };
}

export function sameOverride(
  a: ThemeOverrideState,
  b: ThemeOverrideState,
): boolean {
  return a.override === b.override && a.base === b.base;
}
