import {
  scopedLimits,
  sessionLimit,
  weeklyLimit,
  type UsageLimit,
  type UsageSnapshot,
} from './usage';

export type Segment = 'session' | 'weekly' | 'scoped' | 'reset' | 'spend' | 'plan';
export type BarSeverity = 'normal' | 'warning' | 'error';

export interface FormatOptions {
  segments: Segment[];
  staleAfterMs: number;
  warnAtPercent: number;
  errorAtPercent: number;
}

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/** "4h 52m", "3d 2h", "12m", "<1m", "now". */
export function formatCountdown(remainingMs: number): string {
  if (!Number.isFinite(remainingMs) || remainingMs <= 0) return 'now';
  if (remainingMs < MINUTE) return '<1m';

  if (remainingMs >= DAY) {
    const days = Math.floor(remainingMs / DAY);
    const hours = Math.floor((remainingMs % DAY) / HOUR);
    return hours > 0 ? `${days}d ${hours}h` : `${days}d`;
  }
  if (remainingMs >= HOUR) {
    const hours = Math.floor(remainingMs / HOUR);
    const minutes = Math.floor((remainingMs % HOUR) / MINUTE);
    return minutes > 0 ? `${hours}h ${minutes}m` : `${hours}h`;
  }
  return `${Math.floor(remainingMs / MINUTE)}m`;
}

/** Whole percents, except that a nonzero sliver should never render as "0%". */
export function formatPercent(value: number): string {
  if (value > 0 && value < 0.5) return '<1%';
  if (value < 100 && value >= 99.5) return '99%';
  return `${Math.round(value)}%`;
}

function formatMoney(amount: number, currency: string): string {
  const symbol = currency === 'USD' ? '$' : `${currency} `;
  return `${symbol}${amount.toFixed(2)}`;
}

export function isStale(snapshot: UsageSnapshot, nowMs: number, staleAfterMs: number): boolean {
  return nowMs - snapshot.fetchedAtMs > staleAfterMs;
}

/** The window resetting soonest — what a bare countdown should track. */
export function nextReset(snapshot: UsageSnapshot): UsageLimit | undefined {
  const candidates = snapshot.limits.filter((limit) => limit.resetsAtMs !== undefined);
  if (candidates.length === 0) return undefined;
  return candidates.reduce((a, b) => (a.resetsAtMs! <= b.resetsAtMs! ? a : b));
}

/** The fullest per-model window, which is the one worth surfacing. */
function topScoped(snapshot: UsageSnapshot): UsageLimit | undefined {
  return scopedLimits(snapshot)[0];
}

export function severityFor(
  snapshot: UsageSnapshot | undefined,
  options: FormatOptions,
): BarSeverity {
  if (!snapshot) return 'normal';

  // Trust Claude Code's own severity when it escalates, then fall back to the
  // user's thresholds so the colours still track a plain percentage.
  if (snapshot.limits.some((limit) => limit.severity === 'critical')) return 'error';

  const peak = snapshot.limits.reduce((max, limit) => Math.max(max, limit.percent), 0);
  if (peak >= options.errorAtPercent) return 'error';
  if (peak >= options.warnAtPercent || snapshot.limits.some((l) => l.severity === 'warning')) {
    return 'warning';
  }
  return 'normal';
}

function segmentText(
  segment: Segment,
  snapshot: UsageSnapshot,
  nowMs: number,
): string | undefined {
  switch (segment) {
    case 'session': {
      const limit = sessionLimit(snapshot);
      return limit ? `5h ${formatPercent(limit.percent)}` : undefined;
    }
    case 'weekly': {
      const limit = weeklyLimit(snapshot);
      return limit ? `7d ${formatPercent(limit.percent)}` : undefined;
    }
    case 'scoped': {
      const limit = topScoped(snapshot);
      return limit ? `${limit.scopeName} ${formatPercent(limit.percent)}` : undefined;
    }
    case 'reset': {
      const limit = nextReset(snapshot);
      return limit ? `$(history) ${formatCountdown(limit.resetsAtMs! - nowMs)}` : undefined;
    }
    case 'spend':
      return snapshot.spend
        ? formatMoney(snapshot.spend.usedUsd, snapshot.spend.currency)
        : undefined;
    case 'plan':
      return snapshot.subscriptionType;
  }
}

/** Shown before the first reading lands. */
export const EMPTY_TEXT = '$(pulse) Claude usage —';
/** Shown while the first reading is in flight. */
export const LOADING_TEXT = '$(sync~spin) Claude usage';

export function formatStatusText(
  snapshot: UsageSnapshot | undefined,
  nowMs: number,
  options: FormatOptions,
): string {
  if (!snapshot) return EMPTY_TEXT;
  if (!snapshot.available) return '$(pulse) Claude usage n/a';

  const parts = options.segments
    .map((segment) => segmentText(segment, snapshot, nowMs))
    .filter((part): part is string => Boolean(part));

  if (parts.length === 0) return EMPTY_TEXT;

  const stale = isStale(snapshot, nowMs, options.staleAfterMs) ? ' (stale)' : '';
  return `$(pulse) ${parts.join(' · ')}${stale}`;
}

function timeAgo(ms: number): string {
  if (ms < MINUTE) return 'just now';
  return `${formatCountdown(ms)} ago`;
}

function limitLine(limit: UsageLimit, nowMs: number): string {
  const reset =
    limit.resetsAtMs === undefined
      ? ''
      : ` · resets in ${formatCountdown(limit.resetsAtMs - nowMs)}`;
  const active = limit.isActive ? ' — currently limiting' : '';
  return `- ${limit.label}: **${formatPercent(limit.percent)}**${reset}${active}`;
}

/**
 * Strip the emphasis {@link formatTooltip} adds, for plain-text dialogs.
 * Underscores inside words (model ids, plan names) are left alone.
 */
export function toPlainText(markdown: string): string {
  return markdown
    .replace(/\*\*(.+?)\*\*/g, '$1')
    .replace(/(^|\s)_([^_]+)_(?=[\s.,]|$)/gm, '$1$2');
}

/** Markdown for the status bar tooltip. */
export function formatTooltip(
  snapshot: UsageSnapshot | undefined,
  nowMs: number,
  options: FormatOptions,
  problem?: string,
): string {
  const lines: string[] = ['**Claude Code usage**', ''];

  if (!snapshot) {
    lines.push(problem ?? 'Waiting for the first reading from the Claude Code CLI.');
    return lines.join('\n');
  }

  if (!snapshot.available) {
    lines.push('Plan limits are not available for this login method.');
  } else if (snapshot.limits.length === 0) {
    lines.push('Claude Code reported no usage windows.');
  } else {
    for (const limit of snapshot.limits) lines.push(limitLine(limit, nowMs));
  }

  const details: string[] = [];
  if (snapshot.subscriptionType) details.push(`- Plan: ${snapshot.subscriptionType}`);
  if (snapshot.spend) {
    const { usedUsd, limitUsd, currency, percent, enabled } = snapshot.spend;
    const state = enabled ? '' : ' (off)';
    details.push(
      `- Extra usage: ${formatMoney(usedUsd, currency)} of ${formatMoney(limitUsd, currency)}` +
        ` (${formatPercent(percent)})${state}`,
    );
  }
  if (details.length > 0) lines.push('', ...details);

  lines.push('', `_Updated ${timeAgo(nowMs - snapshot.fetchedAtMs)}_`);
  if (isStale(snapshot, nowMs, options.staleAfterMs)) {
    lines.push('', '_Stale — the last refresh did not succeed._');
  }
  if (problem) lines.push('', `_${problem}_`);
  return lines.join('\n');
}
