import { describe, expect, it } from 'vitest';
import { hasRateLimits, parseResetsAt, parseSnapshot, toSnapshot } from './payload';

/** Shaped after the real payload Claude Code 2.1.222 builds for `statusLine`. */
const SAMPLE = {
  session_id: '00000000-0000-4000-8000-000000000000',
  cwd: '/home/user/project',
  session_name: 'example-session',
  model: { id: 'claude-opus-5', display_name: 'Opus 5' },
  workspace: { current_dir: '/repo', project_dir: '/repo' },
  version: '2.1.222',
  output_style: { name: 'default' },
  cost: {
    total_cost_usd: 1.2345,
    total_duration_ms: 900_000,
    total_api_duration_ms: 420_000,
    total_lines_added: 120,
    total_lines_removed: 8,
  },
  context_window: {
    total_input_tokens: 52_000,
    total_output_tokens: 3_100,
    context_window_size: 200_000,
    used_percentage: 26.5,
    remaining_percentage: 73.5,
  },
  exceeds_200k_tokens: false,
  rate_limits: {
    five_hour: { used_percentage: 1, resets_at: '2026-08-05T13:07:00.000Z' },
    seven_day: { used_percentage: 68, resets_at: '2026-08-09T00:00:00.000Z' },
  },
};

describe('parseResetsAt', () => {
  it('parses ISO-8601 strings', () => {
    expect(parseResetsAt('2026-08-05T13:07:00.000Z')).toBe(Date.parse('2026-08-05T13:07:00.000Z'));
  });

  it('treats small numbers as epoch seconds and large ones as milliseconds', () => {
    expect(parseResetsAt(1_785_936_420)).toBe(1_785_936_420_000);
    expect(parseResetsAt(1_785_936_420_000)).toBe(1_785_936_420_000);
  });

  it('accepts numeric strings', () => {
    expect(parseResetsAt('1785936420')).toBe(1_785_936_420_000);
  });

  it('rejects junk', () => {
    for (const value of [undefined, null, '', '  ', 'soon', Number.NaN, 0, -5, {}]) {
      expect(parseResetsAt(value)).toBeUndefined();
    }
  });
});

describe('toSnapshot', () => {
  it('extracts plan usage, cost and context', () => {
    const snapshot = toSnapshot(SAMPLE, 1_000);
    expect(snapshot).toBeDefined();
    expect(snapshot!.fiveHour).toEqual({
      usedPercent: 1,
      resetsAtMs: Date.parse('2026-08-05T13:07:00.000Z'),
    });
    expect(snapshot!.sevenDay?.usedPercent).toBe(68);
    expect(snapshot!.modelName).toBe('Opus 5');
    expect(snapshot!.costUsd).toBeCloseTo(1.2345);
    expect(snapshot!.contextUsedPercent).toBe(26.5);
    expect(snapshot!.receivedAtMs).toBe(1_000);
  });

  it('falls back to the model id when there is no display name', () => {
    const snapshot = toSnapshot({ model: { id: 'claude-opus-5' } }, 0);
    expect(snapshot!.modelName).toBe('claude-opus-5');
  });

  it('survives a payload with no rate_limits block', () => {
    const { rate_limits: _omitted, ...withoutLimits } = SAMPLE;
    const snapshot = toSnapshot(withoutLimits, 0);
    expect(snapshot).toBeDefined();
    expect(snapshot!.fiveHour).toBeUndefined();
    expect(hasRateLimits(snapshot)).toBe(false);
  });

  it('keeps a window that only carries a reset time', () => {
    const snapshot = toSnapshot(
      { rate_limits: { five_hour: { resets_at: '2026-08-05T13:07:00.000Z' } } },
      0,
    );
    expect(snapshot!.fiveHour?.usedPercent).toBe(0);
    expect(snapshot!.fiveHour?.resetsAtMs).toBeDefined();
  });

  it('clamps negative percentages but preserves overage above 100', () => {
    const snapshot = toSnapshot(
      { rate_limits: { five_hour: { used_percentage: -3 }, seven_day: { used_percentage: 137 } } },
      0,
    );
    expect(snapshot!.fiveHour?.usedPercent).toBe(0);
    expect(snapshot!.sevenDay?.usedPercent).toBe(137);
  });

  it('rejects non-objects', () => {
    for (const value of [null, undefined, 42, 'x', []]) {
      expect(toSnapshot(value, 0)).toBeUndefined();
    }
  });
});

describe('parseSnapshot', () => {
  it('round-trips the serialised payload', () => {
    const snapshot = parseSnapshot(JSON.stringify(SAMPLE), 7);
    expect(snapshot!.sevenDay?.usedPercent).toBe(68);
    expect(hasRateLimits(snapshot)).toBe(true);
  });

  it('returns undefined for a truncated write', () => {
    expect(parseSnapshot(JSON.stringify(SAMPLE).slice(0, 80), 0)).toBeUndefined();
    expect(parseSnapshot('', 0)).toBeUndefined();
  });
});
