import { describe, expect, it } from 'vitest';
import {
  labelFor,
  parseResetsAt,
  scopedLimits,
  sessionLimit,
  toSnapshot,
  weeklyLimit,
} from './usage';

const NOW = Date.parse('2026-08-05T09:21:00.000Z');

/**
 * Trimmed from a real `get_usage` control response (Claude Code 2.1.222) — the
 * `limits` array, the legacy per-window keys and `spend` all together, which is
 * what the CLI actually sends.
 */
const REAL_RESPONSE = {
  session: { total_cost_usd: 0, model_usage: {} },
  subscription_type: 'max',
  rate_limits_available: true,
  rate_limits: {
    five_hour: {
      utilization: 7,
      resets_at: '2026-08-05T13:10:00.520748+00:00',
      limit_dollars: null,
    },
    seven_day: { utilization: 69, resets_at: '2026-08-05T23:00:00.520772+00:00' },
    seven_day_opus: null,
    seven_day_sonnet: null,
    extra_usage: {
      is_enabled: false,
      monthly_limit: 20000,
      used_credits: 7277,
      utilization: 36.385,
      currency: 'USD',
      decimal_places: 2,
      disabled_reason: 'out_of_credits',
    },
    limits: [
      {
        kind: 'session',
        group: 'session',
        percent: 7,
        severity: 'normal',
        resets_at: '2026-08-05T13:10:00.520748+00:00',
        scope: null,
        is_active: false,
      },
      {
        kind: 'weekly_all',
        group: 'weekly',
        percent: 69,
        severity: 'normal',
        resets_at: '2026-08-05T23:00:00.520772+00:00',
        scope: null,
        is_active: false,
      },
      {
        kind: 'weekly_scoped',
        group: 'weekly',
        percent: 100,
        severity: 'critical',
        resets_at: '2026-08-05T22:59:59.567179+00:00',
        scope: { model: { id: null, display_name: 'Fable' }, surface: null },
        is_active: true,
      },
    ],
    spend: {
      used: { amount_minor: 7277, currency: 'USD', exponent: 2 },
      limit: { amount_minor: 20000, currency: 'USD', exponent: 2 },
      percent: 36,
      severity: 'normal',
      enabled: false,
      disabled_reason: 'out_of_credits',
    },
  },
};

describe('parseResetsAt', () => {
  it('accepts the ISO strings Claude Code sends', () => {
    expect(parseResetsAt('2026-08-05T13:10:00.520748+00:00')).toBe(
      Date.parse('2026-08-05T13:10:00.520Z'),
    );
  });

  it('accepts epoch seconds and milliseconds', () => {
    expect(parseResetsAt(1_770_000_000)).toBe(1_770_000_000_000);
    expect(parseResetsAt(1_770_000_000_000)).toBe(1_770_000_000_000);
    expect(parseResetsAt('1770000000')).toBe(1_770_000_000_000);
  });

  it('rejects what it cannot read', () => {
    expect(parseResetsAt(undefined)).toBeUndefined();
    expect(parseResetsAt(null)).toBeUndefined();
    expect(parseResetsAt('')).toBeUndefined();
    expect(parseResetsAt('whenever')).toBeUndefined();
    expect(parseResetsAt(0)).toBeUndefined();
    expect(parseResetsAt(Number.NaN)).toBeUndefined();
  });
});

describe('toSnapshot', () => {
  it('reads a real response', () => {
    const snapshot = toSnapshot(REAL_RESPONSE, NOW)!;

    expect(snapshot.available).toBe(true);
    expect(snapshot.subscriptionType).toBe('max');
    expect(snapshot.fetchedAtMs).toBe(NOW);
    expect(snapshot.limits.map((l) => [l.kind, l.percent])).toEqual([
      ['session', 7],
      ['weekly_scoped', 100],
      ['weekly_all', 69],
    ]);
  });

  it('labels the windows the way the tooltip shows them', () => {
    const snapshot = toSnapshot(REAL_RESPONSE, NOW)!;
    expect(snapshot.limits.map((l) => l.label)).toEqual([
      'Session (5h)',
      'Weekly · Fable',
      'Weekly, all models',
    ]);
  });

  it('picks out the session, all-models and per-model windows', () => {
    const snapshot = toSnapshot(REAL_RESPONSE, NOW)!;

    expect(sessionLimit(snapshot)?.percent).toBe(7);
    expect(weeklyLimit(snapshot)?.percent).toBe(69);
    expect(scopedLimits(snapshot).map((l) => l.scopeName)).toEqual(['Fable']);
  });

  it('keeps the severity and active flags Claude Code assigns', () => {
    const scoped = scopedLimits(toSnapshot(REAL_RESPONSE, NOW)!)[0];
    expect(scoped.severity).toBe('critical');
    expect(scoped.isActive).toBe(true);
  });

  it('converts spend from minor units', () => {
    const spend = toSnapshot(REAL_RESPONSE, NOW)!.spend!;
    expect(spend.usedUsd).toBeCloseTo(72.77);
    expect(spend.limitUsd).toBeCloseTo(200);
    expect(spend.percent).toBe(36);
    expect(spend.enabled).toBe(false);
    expect(spend.disabledReason).toBe('out_of_credits');
  });

  it('falls back to extra_usage when there is no spend block', () => {
    const { spend: _dropped, ...rateLimits } = REAL_RESPONSE.rate_limits;
    const snapshot = toSnapshot({ ...REAL_RESPONSE, rate_limits: rateLimits }, NOW)!;

    expect(snapshot.spend?.usedUsd).toBeCloseTo(72.77);
    expect(snapshot.spend?.limitUsd).toBeCloseTo(200);
    expect(snapshot.spend?.percent).toBeCloseTo(36.385);
  });

  it('falls back to the per-window keys when there is no limits array', () => {
    const { limits: _dropped, ...rateLimits } = REAL_RESPONSE.rate_limits;
    const snapshot = toSnapshot({ ...REAL_RESPONSE, rate_limits: rateLimits }, NOW)!;

    expect(snapshot.limits.map((l) => [l.kind, l.percent])).toEqual([
      ['five_hour', 7],
      ['seven_day', 69],
    ]);
    expect(snapshot.limits[1].resetsAtMs).toBe(Date.parse('2026-08-05T23:00:00.520Z'));
  });

  it('skips null windows in the fallback', () => {
    const snapshot = toSnapshot(
      { rate_limits: { five_hour: { utilization: 4 }, seven_day_opus: null } },
      NOW,
    )!;
    expect(snapshot.limits).toHaveLength(1);
  });

  it('marks a login without plan limits unavailable', () => {
    const snapshot = toSnapshot({ rate_limits_available: false, rate_limits: null }, NOW)!;
    expect(snapshot.available).toBe(false);
    expect(snapshot.limits).toEqual([]);
  });

  it('treats a missing availability flag as available', () => {
    expect(toSnapshot({ rate_limits: { limits: [] } }, NOW)?.available).toBe(true);
  });

  it('rejects junk', () => {
    expect(toSnapshot(undefined, NOW)).toBeUndefined();
    expect(toSnapshot(null, NOW)).toBeUndefined();
    expect(toSnapshot([], NOW)).toBeUndefined();
    expect(toSnapshot('nope', NOW)).toBeUndefined();
  });

  it('drops limit entries with no percentage', () => {
    const snapshot = toSnapshot(
      { rate_limits: { limits: [{ kind: 'session' }, { kind: 'weekly_all', percent: 10 }] } },
      NOW,
    )!;
    expect(snapshot.limits.map((l) => l.kind)).toEqual(['weekly_all']);
  });

  it('clamps a negative percentage but keeps overage above 100', () => {
    const snapshot = toSnapshot(
      {
        rate_limits: {
          limits: [
            { kind: 'session', group: 'session', percent: -3 },
            { kind: 'weekly_all', group: 'weekly', percent: 140 },
          ],
        },
      },
      NOW,
    )!;
    expect(sessionLimit(snapshot)?.percent).toBe(0);
    expect(weeklyLimit(snapshot)?.percent).toBe(140);
  });
});

describe('labelFor', () => {
  it('names the windows this extension knows', () => {
    expect(labelFor('session')).toBe('Session (5h)');
    expect(labelFor('weekly_all')).toBe('Weekly, all models');
    expect(labelFor('seven_day_sonnet')).toBe('Weekly, Sonnet');
  });

  it('uses the model name for a scoped window', () => {
    expect(labelFor('weekly_scoped', 'Opus 5')).toBe('Weekly · Opus 5');
  });

  it('humanises kinds it has never seen', () => {
    expect(labelFor('seven_day_new_thing')).toBe('Seven day new thing');
  });
});
