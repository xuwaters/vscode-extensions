import { describe, expect, it } from 'vitest';
import {
  EMPTY_TEXT,
  formatCountdown,
  formatPercent,
  formatStatusText,
  formatTooltip,
  isStale,
  nextReset,
  severityFor,
  toPlainText,
  type FormatOptions,
} from './format';
import type { UsageLimit, UsageSnapshot } from './usage';

const NOW = Date.parse('2026-08-05T09:21:00.000Z');

const OPTIONS: FormatOptions = {
  segments: ['session', 'weekly', 'reset'],
  staleAfterMs: 30 * 60_000,
  warnAtPercent: 80,
  errorAtPercent: 95,
};

function limit(overrides: Partial<UsageLimit> = {}): UsageLimit {
  return {
    kind: 'session',
    group: 'session',
    label: 'Session (5h)',
    percent: 7,
    severity: 'normal',
    resetsAtMs: NOW + 3 * 3_600_000 + 49 * 60_000,
    isActive: false,
    ...overrides,
  };
}

const WEEKLY = limit({
  kind: 'weekly_all',
  group: 'weekly',
  label: 'Weekly, all models',
  percent: 69,
  resetsAtMs: NOW + 13 * 3_600_000,
});

const SCOPED = limit({
  kind: 'weekly_scoped',
  group: 'weekly',
  label: 'Weekly · Fable',
  scopeName: 'Fable',
  percent: 100,
  severity: 'critical',
  isActive: true,
});

function snapshot(overrides: Partial<UsageSnapshot> = {}): UsageSnapshot {
  return {
    fetchedAtMs: NOW - 5_000,
    available: true,
    subscriptionType: 'max',
    limits: [limit(), WEEKLY],
    spend: {
      usedUsd: 72.77,
      limitUsd: 200,
      currency: 'USD',
      percent: 36,
      enabled: false,
    },
    ...overrides,
  };
}

describe('formatCountdown', () => {
  it('formats the shapes the status bar needs', () => {
    expect(formatCountdown(4 * 3_600_000 + 52 * 60_000)).toBe('4h 52m');
    expect(formatCountdown(3 * 86_400_000 + 2 * 3_600_000)).toBe('3d 2h');
    expect(formatCountdown(3 * 86_400_000)).toBe('3d');
    expect(formatCountdown(2 * 3_600_000)).toBe('2h');
    expect(formatCountdown(12 * 60_000)).toBe('12m');
    expect(formatCountdown(30_000)).toBe('<1m');
    expect(formatCountdown(0)).toBe('now');
    expect(formatCountdown(-5_000)).toBe('now');
  });
});

describe('formatPercent', () => {
  it('never rounds a nonzero sliver away, or up to a full 100%', () => {
    expect(formatPercent(0)).toBe('0%');
    expect(formatPercent(0.2)).toBe('<1%');
    expect(formatPercent(7)).toBe('7%');
    expect(formatPercent(99.7)).toBe('99%');
    expect(formatPercent(100)).toBe('100%');
    expect(formatPercent(140)).toBe('140%');
  });
});

describe('nextReset', () => {
  it('returns the window resetting soonest', () => {
    expect(nextReset(snapshot())?.kind).toBe('session');
  });

  it('ignores windows with no reset time', () => {
    expect(nextReset(snapshot({ limits: [limit({ resetsAtMs: undefined })] }))).toBeUndefined();
  });
});

describe('formatStatusText', () => {
  it('renders the configured segments in order', () => {
    expect(formatStatusText(snapshot(), NOW, OPTIONS)).toBe(
      '$(pulse) 5h 7% · 7d 69% · $(history) 3h 49m',
    );
  });

  it('renders the per-model window, spend and plan when asked', () => {
    const withScoped = snapshot({ limits: [limit(), WEEKLY, SCOPED] });
    const options: FormatOptions = { ...OPTIONS, segments: ['scoped', 'spend', 'plan'] };
    expect(formatStatusText(withScoped, NOW, options)).toBe('$(pulse) Fable 100% · $72.77 · max');
  });

  it('falls back to the placeholder before anything has landed', () => {
    expect(formatStatusText(undefined, NOW, OPTIONS)).toBe(EMPTY_TEXT);
  });

  it('falls back to the placeholder when no segment has data', () => {
    expect(formatStatusText(snapshot({ limits: [], spend: undefined }), NOW, OPTIONS)).toBe(
      EMPTY_TEXT,
    );
  });

  it('says so when the login has no plan limits', () => {
    expect(formatStatusText(snapshot({ available: false }), NOW, OPTIONS)).toBe(
      '$(pulse) Claude usage n/a',
    );
  });

  it('marks a reading that has gone stale', () => {
    expect(formatStatusText(snapshot({ fetchedAtMs: NOW - 45 * 60_000 }), NOW, OPTIONS)).toContain(
      '(stale)',
    );
  });
});

describe('severityFor', () => {
  it('stays normal below the thresholds', () => {
    expect(severityFor(snapshot(), OPTIONS)).toBe('normal');
  });

  it('warns once a window passes warnAtPercent', () => {
    expect(severityFor(snapshot({ limits: [limit({ percent: 82 })] }), OPTIONS)).toBe('warning');
  });

  it('errors once a window passes errorAtPercent', () => {
    expect(severityFor(snapshot({ limits: [limit({ percent: 96 })] }), OPTIONS)).toBe('error');
  });

  it("escalates on Claude Code's own critical severity, whatever the percentage", () => {
    const critical = snapshot({ limits: [limit({ percent: 12, severity: 'critical' })] });
    expect(severityFor(critical, OPTIONS)).toBe('error');
  });

  it('has nothing to say without a reading', () => {
    expect(severityFor(undefined, OPTIONS)).toBe('normal');
  });
});

describe('isStale', () => {
  it('measures from the last successful refresh', () => {
    expect(isStale(snapshot(), NOW, OPTIONS.staleAfterMs)).toBe(false);
    expect(isStale(snapshot({ fetchedAtMs: NOW - 31 * 60_000 }), NOW, OPTIONS.staleAfterMs)).toBe(
      true,
    );
  });
});

describe('formatTooltip', () => {
  it('lists every window with its countdown', () => {
    const tooltip = formatTooltip(snapshot(), NOW, OPTIONS);
    expect(tooltip).toContain('- Session (5h): **7%** · resets in 3h 49m');
    expect(tooltip).toContain('- Weekly, all models: **69%** · resets in 13h');
    expect(tooltip).toContain('- Plan: max');
    expect(tooltip).toContain('- Extra usage: $72.77 of $200.00 (36%) (off)');
    expect(tooltip).toContain('_Updated just now_');
  });

  it('flags the window that is actually limiting', () => {
    expect(formatTooltip(snapshot({ limits: [SCOPED] }), NOW, OPTIONS)).toContain(
      '— currently limiting',
    );
  });

  it('explains itself before the first reading', () => {
    expect(formatTooltip(undefined, NOW, OPTIONS)).toContain('Waiting for the first reading');
  });

  it('shows the failure instead when there is one and no reading', () => {
    expect(formatTooltip(undefined, NOW, OPTIONS, 'Could not find the CLI')).toContain(
      'Could not find the CLI',
    );
  });

  it('keeps showing the last reading alongside a failure', () => {
    const tooltip = formatTooltip(snapshot(), NOW, OPTIONS, 'Last refresh failed: timeout');
    expect(tooltip).toContain('- Session (5h)');
    expect(tooltip).toContain('_Last refresh failed: timeout_');
  });

  it('says when the login has no plan limits', () => {
    expect(formatTooltip(snapshot({ available: false }), NOW, OPTIONS)).toContain(
      'not available for this login method',
    );
  });

  it('notes staleness', () => {
    expect(formatTooltip(snapshot({ fetchedAtMs: NOW - 45 * 60_000 }), NOW, OPTIONS)).toContain(
      '_Stale',
    );
  });
});

describe('toPlainText', () => {
  it('drops the emphasis but leaves underscores inside words alone', () => {
    expect(toPlainText('- Session (5h): **7%**')).toBe('- Session (5h): 7%');
    expect(toPlainText('_Updated just now_')).toBe('Updated just now');
    expect(toPlainText('- Kind: weekly_scoped')).toBe('- Kind: weekly_scoped');
  });
});
