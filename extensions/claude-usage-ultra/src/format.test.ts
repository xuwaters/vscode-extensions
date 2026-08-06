import { describe, expect, it } from 'vitest';
import {
  EMPTY_TEXT,
  formatCountdown,
  formatPercent,
  formatPlan,
  formatStatusText,
  formatTooltip,
  isStale,
  levelFor,
  nextReset,
  toPlainText,
  type FormatOptions,
} from './format';
import { creditsResetMs, type UsageLimit, type UsageSnapshot, type UsageSpend } from './usage';

const NOW = Date.parse('2026-08-05T09:21:00.000Z');

/** The em space {@link formatStatusText} puts between chunks. */
const GAP = ' ';

/**
 * The countdown to the credit renewal. Derived rather than written out, because
 * the first of next month is a local-time boundary and the figure moves with
 * the machine's zone; the date arithmetic itself is pinned in `usage.test.ts`.
 */
const RENEWAL = formatCountdown(creditsResetMs(NOW) - NOW);

const OK = '🟢';
const NOTICE = '🟡';
const WARN = '🟠';
const CRITICAL = '🔴';

const OPTIONS: FormatOptions = {
  segments: ['session', 'weekly', 'reset'],
  label: 'Claude',
  staleAfterMs: 30 * 60_000,
  noticeAtPercent: 50,
  warnAtPercent: 80,
  criticalAtPercent: 95,
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

describe('formatPlan', () => {
  it('title-cases the API slug', () => {
    expect(formatPlan('max')).toBe('Max');
    expect(formatPlan('pro')).toBe('Pro');
    expect(formatPlan('max_5x')).toBe('Max 5x');
    expect(formatPlan('Enterprise')).toBe('Enterprise');
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
  it('renders the label, then the configured segments in order', () => {
    expect(formatStatusText(snapshot(), NOW, OPTIONS)).toBe(
      `$(pulse) Claude${GAP}${OK} Session 7% / 3h 49m · ${NOTICE} Weekly 69% / 13h`,
    );
  });

  it('drops the label when it is blank', () => {
    expect(formatStatusText(snapshot(), NOW, { ...OPTIONS, label: '  ' })).toBe(
      `$(pulse) ${OK} Session 7% / 3h 49m · ${NOTICE} Weekly 69% / 13h`,
    );
  });

  it('renders the per-model window, credits and plan when asked', () => {
    const withScoped = snapshot({ limits: [limit(), WEEKLY, SCOPED] });
    const options: FormatOptions = {
      ...OPTIONS,
      label: '',
      segments: ['scoped', 'spend', 'plan'],
    };
    expect(formatStatusText(withScoped, NOW, options)).toBe(
      `$(pulse) ${CRITICAL} Fable 100% / 3h 49m${GAP}$(credit-card) $72.77 / $200${GAP}Max`,
    );
  });

  it('renders the spend against its cap, without a cap ending in ".00"', () => {
    const options: FormatOptions = { ...OPTIONS, label: '', segments: ['spend'] };
    const spending = (spend: Partial<UsageSpend>) =>
      formatStatusText(snapshot({ spend: { ...snapshot().spend!, ...spend } }), NOW, options);

    expect(spending({ usedUsd: 384.87, limitUsd: 4000 })).toBe(
      '$(pulse) $(credit-card) $384.87 / $4000',
    );
    expect(spending({ usedUsd: 72, limitUsd: 200 })).toBe('$(pulse) $(credit-card) $72 / $200');
    expect(spending({ usedUsd: 5.5, limitUsd: 99.5 })).toBe(
      '$(pulse) $(credit-card) $5.50 / $99.50',
    );
    // A currency with no symbol names itself once, not on both sides.
    expect(spending({ currency: 'EUR' })).toBe('$(pulse) $(credit-card) EUR 72.77 / 200');
    // An uncapped account has nothing to compare against.
    expect(spending({ limitUsd: 0 })).toBe('$(pulse) $(credit-card) $72.77');
  });

  it('omits the plan when the CLI did not report one', () => {
    const options: FormatOptions = { ...OPTIONS, label: '', segments: ['plan', 'session'] };
    expect(formatStatusText(snapshot({ subscriptionType: undefined }), NOW, options)).toBe(
      `$(pulse) ${OK} Session 7% / 3h 49m`,
    );
  });

  it('shows the default segments — plan, Fable and credits included', () => {
    const full = snapshot({ limits: [limit(), WEEKLY, SCOPED] });
    const options: FormatOptions = {
      ...OPTIONS,
      segments: ['plan', 'session', 'weekly', 'scoped', 'spend', 'reset'],
    };
    // Fable resets with the session window here, and the reset segment has
    // nothing the windows have not already said — both go quiet.
    expect(formatStatusText(full, NOW, options)).toBe(
      `$(pulse) Claude Max${GAP}${OK} Session 7% / 3h 49m · ${NOTICE} Weekly 69% / 13h` +
        ` · ${CRITICAL} Fable 100%${GAP}$(credit-card) $72.77 / $200`,
    );
  });

  it('counts down each window beside its own percentage', () => {
    const options: FormatOptions = { ...OPTIONS, label: '', segments: ['session', 'weekly'] };
    const week = snapshot({
      limits: [limit(), limit({ ...WEEKLY, resetsAtMs: NOW + 3 * 86_400_000 + 2 * 3_600_000 })],
    });
    expect(formatStatusText(week, NOW, options)).toBe(
      `$(pulse) ${OK} Session 7% / 3h 49m · ${NOTICE} Weekly 69% / 3d 2h`,
    );
  });

  it('writes a shared countdown once — a per-model window resets with the weekly', () => {
    const options: FormatOptions = { ...OPTIONS, label: '', segments: ['weekly', 'scoped'] };
    const shared = snapshot({ limits: [WEEKLY, { ...SCOPED, resetsAtMs: WEEKLY.resetsAtMs }] });
    expect(formatStatusText(shared, NOW, options)).toBe(
      `$(pulse) ${NOTICE} Weekly 69% / 13h · ${CRITICAL} Fable 100%`,
    );
  });

  it('leaves the standalone countdowns to the reset segment when no window is shown', () => {
    const options: FormatOptions = { ...OPTIONS, label: '', segments: ['reset'] };
    expect(formatStatusText(snapshot(), NOW, options)).toBe(
      '$(pulse) $(history) Session 3h 49m · Weekly 13h',
    );
  });

  it('counts down whichever of the two windows reported a reset', () => {
    const options: FormatOptions = { ...OPTIONS, label: '', segments: ['reset'] };
    const noWeekly = snapshot({ limits: [limit(), limit({ ...WEEKLY, resetsAtMs: undefined })] });
    expect(formatStatusText(noWeekly, NOW, options)).toBe('$(pulse) $(history) Session 3h 49m');
  });

  it('falls back to the next reset when neither named window has one', () => {
    const options: FormatOptions = { ...OPTIONS, label: '', segments: ['reset'] };
    const scopedOnly = snapshot({ limits: [SCOPED] });
    expect(formatStatusText(scopedOnly, NOW, options)).toBe('$(pulse) $(history) Fable 3h 49m');
  });

  it('counts the credits down instead on a plan with no windows', () => {
    const options: FormatOptions = { ...OPTIONS, label: '', segments: ['reset'] };
    // An Enterprise plan: no session or weekly window, only credits.
    expect(formatStatusText(snapshot({ limits: [] }), NOW, options)).toBe(
      `$(pulse) $(history) Credits ${RENEWAL}`,
    );
  });

  it('leaves the credit renewal off a bar that already has a window clock', () => {
    const options: FormatOptions = { ...OPTIONS, label: '', segments: ['reset'] };
    expect(formatStatusText(snapshot(), NOW, options)).not.toContain('Credits');
  });

  it('says nothing when a plan has neither windows nor credits', () => {
    const options: FormatOptions = { ...OPTIONS, label: '', segments: ['plan', 'reset'] };
    expect(formatStatusText(snapshot({ limits: [], spend: undefined }), NOW, options)).toBe(
      '$(pulse) Max',
    );
  });

  it('shows the default segments on a plan billed in credits', () => {
    const enterprise = snapshot({ subscriptionType: 'enterprise', limits: [] });
    const options: FormatOptions = {
      ...OPTIONS,
      segments: ['plan', 'session', 'weekly', 'scoped', 'spend', 'reset'],
    };
    expect(formatStatusText(enterprise, NOW, options)).toBe(
      `$(pulse) Claude Enterprise${GAP}$(credit-card) $72.77 / $200${GAP}$(history) Credits ${RENEWAL}`,
    );
  });

  it('opens a gap wherever the configured order crosses chunks', () => {
    const options: FormatOptions = {
      ...OPTIONS,
      label: '',
      segments: ['session', 'spend', 'weekly'],
    };
    expect(formatStatusText(snapshot(), NOW, options)).toBe(
      `$(pulse) ${OK} Session 7% / 3h 49m${GAP}$(credit-card) $72.77 / $200` +
        `${GAP}${NOTICE} Weekly 69% / 13h`,
    );
  });

  it('dots every window on its own percentage, not the worst one on screen', () => {
    const full = snapshot({ limits: [limit({ percent: 84 }), WEEKLY, SCOPED] });
    const options: FormatOptions = {
      ...OPTIONS,
      label: '',
      segments: ['session', 'weekly', 'scoped'],
    };
    expect(formatStatusText(full, NOW, options)).toBe(
      `$(pulse) ${WARN} Session 84% / 3h 49m · ${NOTICE} Weekly 69% / 13h · ${CRITICAL} Fable 100%`,
    );
  });

  it('dots a quiet window green rather than leaving a hole', () => {
    const calm = snapshot({ limits: [limit(), limit({ ...WEEKLY, percent: 12 })] });
    const options: FormatOptions = { ...OPTIONS, label: '', segments: ['session', 'weekly'] };
    expect(formatStatusText(calm, NOW, options)).toBe(
      `$(pulse) ${OK} Session 7% / 3h 49m · ${OK} Weekly 12% / 13h`,
    );
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

describe('levelFor', () => {
  it('steps up at each threshold, and stays normal below the first', () => {
    expect(levelFor(limit({ percent: 49 }), OPTIONS)).toBe('normal');
    expect(levelFor(limit({ percent: 50 }), OPTIONS)).toBe('notice');
    expect(levelFor(limit({ percent: 70 }), OPTIONS)).toBe('notice');
    expect(levelFor(limit({ percent: 80 }), OPTIONS)).toBe('warning');
    expect(levelFor(limit({ percent: 95 }), OPTIONS)).toBe('critical');
    expect(levelFor(limit({ percent: 140 }), OPTIONS)).toBe('critical');
  });

  it("escalates on Claude Code's own severity, whatever the percentage", () => {
    expect(levelFor(limit({ percent: 12, severity: 'critical' }), OPTIONS)).toBe('critical');
    expect(levelFor(limit({ percent: 12, severity: 'warning' }), OPTIONS)).toBe('warning');
  });

  it('honours thresholds the user has moved', () => {
    const eager: FormatOptions = { ...OPTIONS, noticeAtPercent: 10, warnAtPercent: 20 };
    expect(levelFor(limit({ percent: 15 }), eager)).toBe('notice');
    expect(levelFor(limit({ percent: 25 }), eager)).toBe('warning');

    const off: FormatOptions = { ...OPTIONS, noticeAtPercent: 100 };
    expect(levelFor(limit({ percent: 70 }), off)).toBe('normal');
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
    expect(tooltip).toContain(`- ${OK} Session (5h): **7%** · resets in 3h 49m`);
    expect(tooltip).toContain(`- ${NOTICE} Weekly, all models: **69%** · resets in 13h`);
    expect(tooltip).toContain('- Plan: Max');
    expect(tooltip).toContain(`- Extra usage: $72.77 of $200.00 (36%) (off) · renews in ${RENEWAL}`);
    expect(tooltip).toContain('_Updated just now_');
  });

  it('explains a plan that has credits instead of windows', () => {
    const tooltip = formatTooltip(snapshot({ limits: [] }), NOW, OPTIONS);
    expect(tooltip).toContain('no session or weekly windows');
    expect(tooltip).toContain(`renews in ${RENEWAL}`);
  });

  it('still calls an empty reading empty when there are no credits either', () => {
    expect(formatTooltip(snapshot({ limits: [], spend: undefined }), NOW, OPTIONS)).toContain(
      'reported no usage windows',
    );
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
    expect(tooltip).toContain(`- ${OK} Session (5h)`);
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
