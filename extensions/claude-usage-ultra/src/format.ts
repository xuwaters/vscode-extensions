import type { UsageSnapshot, UsageWindow } from './types';

export type Segment = 'session' | 'weekly' | 'reset' | 'cost' | 'context' | 'model';
export type Severity = 'normal' | 'warning' | 'error';

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

export function isStale(snapshot: UsageSnapshot, nowMs: number, staleAfterMs: number): boolean {
  return nowMs - snapshot.receivedAtMs > staleAfterMs;
}

/** The window resetting soonest — what a bare "resets in ..." should count down. */
export function nextReset(snapshot: UsageSnapshot): UsageWindow | undefined {
  const candidates = [snapshot.fiveHour, snapshot.sevenDay].filter(
    (w): w is UsageWindow => w?.resetsAtMs !== undefined,
  );
  if (candidates.length === 0) return undefined;
  return candidates.reduce((a, b) => (a.resetsAtMs! <= b.resetsAtMs! ? a : b));
}

export function severityFor(
  snapshot: UsageSnapshot | undefined,
  options: FormatOptions,
): Severity {
  if (!snapshot) return 'normal';
  const peak = Math.max(snapshot.fiveHour?.usedPercent ?? 0, snapshot.sevenDay?.usedPercent ?? 0);
  if (peak >= options.errorAtPercent) return 'error';
  if (peak >= options.warnAtPercent) return 'warning';
  return 'normal';
}

function segmentText(
  segment: Segment,
  snapshot: UsageSnapshot,
  nowMs: number,
): string | undefined {
  switch (segment) {
    case 'session':
      return snapshot.fiveHour ? `5h ${formatPercent(snapshot.fiveHour.usedPercent)}` : undefined;
    case 'weekly':
      return snapshot.sevenDay ? `7d ${formatPercent(snapshot.sevenDay.usedPercent)}` : undefined;
    case 'reset': {
      const window = nextReset(snapshot);
      return window ? `$(history) ${formatCountdown(window.resetsAtMs! - nowMs)}` : undefined;
    }
    case 'cost':
      return snapshot.costUsd === undefined ? undefined : `$${snapshot.costUsd.toFixed(2)}`;
    case 'context':
      return snapshot.contextUsedPercent === undefined
        ? undefined
        : `ctx ${formatPercent(snapshot.contextUsedPercent)}`;
    case 'model':
      return snapshot.modelName;
  }
}

/** Placeholder shown before any payload has landed. */
export const EMPTY_TEXT = '$(pulse) Claude usage —';

export function formatStatusText(
  snapshot: UsageSnapshot | undefined,
  nowMs: number,
  options: FormatOptions,
): string {
  if (!snapshot) return EMPTY_TEXT;

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

function windowLine(label: string, window: UsageWindow | undefined, nowMs: number): string {
  if (!window) return `- ${label}: _not reported_`;
  const reset =
    window.resetsAtMs === undefined
      ? ''
      : ` · resets in ${formatCountdown(window.resetsAtMs - nowMs)}`;
  return `- ${label}: **${formatPercent(window.usedPercent)}** used${reset}`;
}

/**
 * Strip the emphasis {@link formatTooltip} adds, for plain-text dialogs.
 * Underscores inside words (session names, model ids) are left alone.
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
  bridgeInstalled: boolean,
): string {
  const lines: string[] = ['**Claude Code usage**', ''];

  if (!snapshot) {
    lines.push(
      bridgeInstalled
        ? 'Bridge installed, but no status line payload has arrived yet. Claude Code only runs the status line command for sessions with a terminal UI.'
        : 'Not set up yet. Run **Claude Usage Ultra: Install Status Line Bridge**.',
    );
    return lines.join('\n');
  }

  lines.push(windowLine('Session (5h)', snapshot.fiveHour, nowMs));
  lines.push(windowLine('Weekly, all models (7d)', snapshot.sevenDay, nowMs));

  if (!snapshot.fiveHour && !snapshot.sevenDay) {
    lines.push('', 'Claude Code has not reported plan limits yet for this session.');
  }

  const details: string[] = [];
  if (snapshot.modelName) details.push(`- Model: ${snapshot.modelName}`);
  if (snapshot.costUsd !== undefined) {
    details.push(`- Session cost: $${snapshot.costUsd.toFixed(2)}`);
  }
  if (snapshot.contextUsedPercent !== undefined) {
    const size = snapshot.contextWindowSize
      ? ` of ${Math.round(snapshot.contextWindowSize / 1000)}k`
      : '';
    details.push(`- Context: ${formatPercent(snapshot.contextUsedPercent)}${size}`);
  }
  if (snapshot.sessionName) details.push(`- Session: ${snapshot.sessionName}`);
  if (details.length > 0) lines.push('', ...details);

  lines.push('', `_Updated ${timeAgo(nowMs - snapshot.receivedAtMs)}_`);
  if (isStale(snapshot, nowMs, options.staleAfterMs)) {
    lines.push('', '_Stale — no Claude Code session has reported since._');
  }
  lines.push('', 'Per-model weekly limits are not available; Claude Code does not send them here.');
  return lines.join('\n');
}
