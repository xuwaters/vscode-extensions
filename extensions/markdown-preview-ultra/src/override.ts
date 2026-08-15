/**
 * The rule behind the preview's in-page switches — light/dark, and the font
 * the page is set in.
 *
 * Kept free of the `vscode` module so the rule itself is testable; the state it
 * is applied to lives in `overrideStore.ts`.
 *
 * A switch is a deviation from the setting it stands for (`theme`, `font`),
 * never a write to it. It holds only while the value it was made against is
 * still the configured one: changing the setting is an explicit choice and
 * retires the switch (that is also the way back to `auto`).
 */

/** An in-page choice plus the configured value it was made against. */
export interface OverrideState<T extends string> {
  override?: T;
  base?: T;
}

/** Drop an override whose configured value has changed under it. */
export function reconcileOverride<T extends string>(
  stored: OverrideState<T>,
  configured: T,
): OverrideState<T> {
  if (stored.override === undefined) return {};
  return stored.base === configured ? stored : {};
}

/**
 * The state left behind by flipping the switch to `chosen`. Landing back on the
 * configured value retires the override entirely, rather than recording an
 * override that happens to agree with it.
 */
export function chooseOverride<T extends string>(
  chosen: T,
  configured: T,
): OverrideState<T> {
  return chosen === configured ? {} : { override: chosen, base: configured };
}

export function sameOverride<T extends string>(
  a: OverrideState<T>,
  b: OverrideState<T>,
): boolean {
  return a.override === b.override && a.base === b.base;
}
