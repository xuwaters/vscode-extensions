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
import type { UsageSnapshot } from './types';

const NOW = Date.parse('2026-08-05T08:15:00.000Z');

const OPTIONS: FormatOptions = {
  segments: ['session', 'weekly', 'reset'],
  staleAfterMs: 20 * 60_000,
  warnAtPercent: 80,
  errorAtPercent: 95,
};

function snapshot(overrides: Partial<UsageSnapshot> = {}): UsageSnapshot {
  return {
    receivedAtMs: NOW - 5_000,
    modelName: 'Opus 5',
    fiveHour: { usedPercent: 1, resetsAtMs: NOW + 4 * 3_600_000 + 52 * 60_000 },
    sevenDay: { usedPercent: 68, resetsAtMs: NOW + 3 * 86_400_000 },
    costUsd: 1.2345,
    contextUsedPercent: 26.5,
    contextWindowSize: 200_000,
    ...overrides,
  };
}

describe('formatCountdown', () => {
  it('formats the shapes the status bar needs', () => {
    expect(formatCountdown(4 * 3_600_000 + 52 * 60_000)).toBe('4h 52m');
    expect(formatCountdown(3 * 3_600_000)).toBe('3h');
    expect(formatCountdown(12 * 60_000)).toBe('12m');
    expect(formatCountdown(30_000)).toBe('<1m');
    expect(formatCountdown(0)).toBe('now');
    expect(formatCountdown(-1_000)).toBe('now');
    expect(formatCountdown(3 * 86_400_000 + 2 * 3_600_000)).toBe('3d 2h');
    expect(formatCountdown(2 * 86_400_000)).toBe('2d');
  });
});

describe('formatPercent', () => {
  it('never rounds a nonzero sliver down to 0%', () => {
    expect(formatPercent(0)).toBe('0%');
    expect(formatPercent(0.2)).toBe('<1%');
    expect(formatPercent(1)).toBe('1%');
    expect(formatPercent(68.4)).toBe('68%');
  });

  it('never rounds an incomplete window up to 100%', () => {
    expect(formatPercent(99.7)).toBe('99%');
    expect(formatPercent(100)).toBe('100%');
    expect(formatPercent(137)).toBe('137%');
  });
});

describe('formatStatusText', () => {
  it('renders the default segments', () => {
    expect(formatStatusText(snapshot(), NOW, OPTIONS)).toBe(
      '$(pulse) 5h 1% · 7d 68% · $(history) 4h 52m',
    );
  });

  it('renders optional segments in the configured order', () => {
    const text = formatStatusText(snapshot(), NOW, {
      ...OPTIONS,
      segments: ['model', 'cost', 'context', 'session'],
    });
    expect(text).toBe('$(pulse) Opus 5 · $1.23 · ctx 27% · 5h 1%');
  });

  it('drops segments the payload has no data for', () => {
    const text = formatStatusText(
      snapshot({ sevenDay: undefined, costUsd: undefined }),
      NOW,
      OPTIONS,
    );
    expect(text).toBe('$(pulse) 5h 1% · $(history) 4h 52m');
  });

  it('marks a reading stale', () => {
    const text = formatStatusText(snapshot({ receivedAtMs: NOW - 60 * 60_000 }), NOW, OPTIONS);
    expect(text).toContain('(stale)');
  });

  it('falls back to a placeholder with no snapshot or no usable segments', () => {
    expect(formatStatusText(undefined, NOW, OPTIONS)).toBe(EMPTY_TEXT);
    expect(
      formatStatusText(
        snapshot({ fiveHour: undefined, sevenDay: undefined }),
        NOW,
        { ...OPTIONS, segments: ['session', 'weekly', 'reset'] },
      ),
    ).toBe(EMPTY_TEXT);
  });
});

describe('nextReset', () => {
  it('picks the window resetting soonest', () => {
    const current = snapshot();
    expect(nextReset(current)).toBe(current.fiveHour);
  });

  it('falls back to the weekly window', () => {
    const result = nextReset(snapshot({ fiveHour: { usedPercent: 1 } }));
    expect(result!.resetsAtMs).toBe(NOW + 3 * 86_400_000);
  });

  it('returns undefined when nothing carries a reset time', () => {
    expect(nextReset(snapshot({ fiveHour: undefined, sevenDay: undefined }))).toBeUndefined();
  });
});

describe('severityFor', () => {
  it('escalates on the higher of the two windows', () => {
    expect(severityFor(snapshot(), OPTIONS)).toBe('normal');
    expect(severityFor(snapshot({ sevenDay: { usedPercent: 82 } }), OPTIONS)).toBe('warning');
    expect(severityFor(snapshot({ fiveHour: { usedPercent: 97 } }), OPTIONS)).toBe('error');
    expect(severityFor(undefined, OPTIONS)).toBe('normal');
  });
});

describe('isStale', () => {
  it('uses the configured window', () => {
    expect(isStale(snapshot(), NOW, 20 * 60_000)).toBe(false);
    expect(isStale(snapshot({ receivedAtMs: NOW - 21 * 60_000 }), NOW, 20 * 60_000)).toBe(true);
  });
});

describe('toPlainText', () => {
  it('drops emphasis markers used by the tooltip', () => {
    expect(toPlainText('Session (5h): **1%** used')).toBe('Session (5h): 1% used');
    expect(toPlainText('_Updated just now_')).toBe('Updated just now');
  });

  it('leaves underscores inside identifiers alone', () => {
    expect(toPlainText('- Session: my_session_name')).toBe('- Session: my_session_name');
    expect(toPlainText('- Model: claude_opus_5')).toBe('- Model: claude_opus_5');
  });

  it('strips every line of a real tooltip', () => {
    const plain = toPlainText(formatTooltip(snapshot(), NOW, OPTIONS, true));
    expect(plain).not.toContain('**');
    expect(plain).toContain('Session (5h): 1% used');
    expect(plain).toContain('Updated just now');
  });
});

describe('formatTooltip', () => {
  it('reports both windows and their resets', () => {
    const tooltip = formatTooltip(snapshot(), NOW, OPTIONS, true);
    expect(tooltip).toContain('Session (5h): **1%** used · resets in 4h 52m');
    expect(tooltip).toContain('Weekly, all models (7d): **68%** used · resets in 3d');
    expect(tooltip).toContain('Session cost: $1.23');
    expect(tooltip).toContain('Per-model weekly limits are not available');
  });

  it('explains the two empty states differently', () => {
    expect(formatTooltip(undefined, NOW, OPTIONS, false)).toContain('Install Status Line Bridge');
    expect(formatTooltip(undefined, NOW, OPTIONS, true)).toContain('no status line payload');
  });

  it('calls out a payload that carried no plan limits', () => {
    const tooltip = formatTooltip(
      snapshot({ fiveHour: undefined, sevenDay: undefined }),
      NOW,
      OPTIONS,
      true,
    );
    expect(tooltip).toContain('has not reported plan limits yet');
  });
});
