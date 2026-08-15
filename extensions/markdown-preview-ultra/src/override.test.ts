import { describe, expect, it } from 'vitest';
import { chooseOverride, reconcileOverride, sameOverride } from './override';

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

  it('is a no-op without an override', () => {
    expect(reconcileOverride({}, 'github-light')).toEqual({});
    expect(reconcileOverride({ base: 'github-light' }, 'auto')).toEqual({});
  });

  it('holds the font switch to the same rule', () => {
    const stored = { override: 'monospace', base: 'proportional' } as const;
    expect(reconcileOverride(stored, 'proportional')).toEqual(stored);
    expect(reconcileOverride(stored, 'monospace')).toEqual({});
  });
});

describe('chooseOverride', () => {
  it('records the theme it deviates from', () => {
    expect(chooseOverride('github-dark', 'auto')).toEqual({
      override: 'github-dark',
      base: 'auto',
    });
  });

  it('retires the override on landing back at the configured theme', () => {
    expect(chooseOverride('github-light', 'github-light')).toEqual({});
  });

  it('records a font choice the same way', () => {
    expect(chooseOverride('monospace', 'proportional')).toEqual({
      override: 'monospace',
      base: 'proportional',
    });
    expect(chooseOverride('monospace', 'monospace')).toEqual({});
  });
});

describe('sameOverride', () => {
  it('compares both halves of the state', () => {
    const state = { override: 'github-dark', base: 'auto' } as const;
    expect(sameOverride(state, { ...state })).toBe(true);
    expect(sameOverride(state, { override: 'github-dark' })).toBe(false);
    expect(sameOverride({}, {})).toBe(true);
  });
});
