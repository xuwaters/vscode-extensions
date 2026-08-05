import {
  scopedLimits,
  sessionLimit,
  weeklyLimit,
  type UsageLimit,
  type UsageSnapshot,
} from './usage';

export type Segment = 'session' | 'weekly' | 'scoped' | 'reset' | 'spend' | 'plan';
type BarSeverity = 'normal' | 'warning' | 'error';

/** A window loud enough to be lifted out of the run and rendered on its own. */
export interface Alert {
  limit: UsageLimit;
  severity: 'warning' | 'error';
}

export interface FormatOptions {
  segments: Segment[];
  /** Prefix naming the item, so the numbers are not anonymous. Empty hides it. */
  label: string;
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

/**
 * The API sends the plan as a lowercase slug — "max", "max_5x". Title-case it
 * so the status bar reads like the plan's name rather than a field value.
 */
export function formatPlan(subscriptionType: string): string {
  return subscriptionType
    .replace(/[_-]+/g, ' ')
    .trim()
    .replace(/\b\w/g, (character) => character.toUpperCase());
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

/** The fullest per-model window other than one already shown elsewhere. */
function nextScoped(snapshot: UsageSnapshot, exclude: UsageLimit): UsageLimit | undefined {
  return scopedLimits(snapshot).find((limit) => limit !== exclude);
}

/** How loudly one window should be shown, given the user's thresholds. */
function severityOf(limit: UsageLimit, options: FormatOptions): BarSeverity {
  // Trust Claude Code's own severity when it escalates, then fall back to the
  // user's thresholds so the colours still track a plain percentage.
  if (limit.severity === 'critical' || limit.percent >= options.errorAtPercent) return 'error';
  if (limit.severity === 'warning' || limit.percent >= options.warnAtPercent) return 'warning';
  return 'normal';
}

/**
 * The single window worth colouring: the loudest, and among equals the fullest.
 * Undefined while everything sits below the thresholds — which is exactly when
 * the bar should stay quiet. Colouring one window rather than the whole reading
 * is the point: a full per-model window used to turn every number red.
 */
export function alertFor(
  snapshot: UsageSnapshot | undefined,
  options: FormatOptions,
): Alert | undefined {
  if (!snapshot?.available) return undefined;

  let loudest: Alert | undefined;
  for (const limit of snapshot.limits) {
    const severity = severityOf(limit, options);
    if (severity === 'normal') continue;
    if (loudest === undefined || outranks({ limit, severity }, loudest)) {
      loudest = { limit, severity };
    }
  }
  return loudest;
}

/** Error beats warning; within a severity, the fuller window wins. */
function outranks(candidate: Alert, held: Alert): boolean {
  if (candidate.severity !== held.severity) return candidate.severity === 'error';
  return candidate.limit.percent > held.limit.percent;
}

/** Text for the alert item. Short, because the colour is doing the shouting. */
export function formatAlertText(alert: Alert): string {
  return windowText(alert.limit);
}

/** How a window names itself in the bar: "5h", "7d", "Fable". */
function shortLabel(limit: UsageLimit): string {
  if (limit.scopeName) return limit.scopeName;
  if (limit.group === 'session') return '5h';
  if (limit.group === 'weekly') return '7d';
  return limit.label;
}

function windowText(limit: UsageLimit): string {
  return `${shortLabel(limit)} ${formatPercent(limit.percent)}`;
}

function segmentText(
  segment: Segment,
  snapshot: UsageSnapshot,
  nowMs: number,
  alerted: UsageLimit | undefined,
): string | undefined {
  switch (segment) {
    case 'session': {
      const limit = sessionLimit(snapshot);
      return limit && limit !== alerted ? windowText(limit) : undefined;
    }
    case 'weekly': {
      const limit = weeklyLimit(snapshot);
      return limit && limit !== alerted ? windowText(limit) : undefined;
    }
    case 'scoped': {
      // The alert item already carries the fullest one, so show the runner-up
      // here rather than dropping the per-model reading altogether.
      const limit = alerted === undefined ? topScoped(snapshot) : nextScoped(snapshot, alerted);
      return limit ? windowText(limit) : undefined;
    }
    case 'reset': {
      const limit = nextReset(snapshot);
      return limit ? `$(history) ${formatCountdown(limit.resetsAtMs! - nowMs)}` : undefined;
    }
    case 'spend':
      // The card icon is what marks this as money rather than one more percentage.
      return snapshot.spend
        ? `$(credit-card) ${formatMoney(snapshot.spend.usedUsd, snapshot.spend.currency)}`
        : undefined;
    case 'plan':
      return snapshot.subscriptionType ? formatPlan(snapshot.subscriptionType) : undefined;
  }
}

/** Shown before the first reading lands. */
export const EMPTY_TEXT = '$(pulse) Claude usage —';
/** Shown while the first reading is in flight. */
export const LOADING_TEXT = '$(sync~spin) Claude usage';

/**
 * Which visual chunk a segment belongs to. Adjacent segments from the same
 * chunk sit tight together; a chunk boundary opens into a gap. Six numbers
 * separated by six identical dots read as mush — the gaps give the eye three
 * or four places to land instead of one long run.
 */
const SEGMENT_CHUNK: Record<Segment, string> = {
  plan: 'identity',
  session: 'windows',
  weekly: 'windows',
  scoped: 'windows',
  spend: 'money',
  reset: 'time',
};

/** An em space: wide enough to read as a gap, and it survives HTML collapsing
 * the way a run of plain spaces would not. */
const CHUNK_GAP = ' ';

interface Piece {
  chunk: string;
  text: string;
}

/** Tight within a chunk, a gap between chunks. */
function joinChunks(pieces: Piece[]): string {
  const groups: Array<{ chunk: string; texts: string[] }> = [];
  for (const piece of pieces) {
    const open = groups[groups.length - 1];
    if (open && open.chunk === piece.chunk) open.texts.push(piece.text);
    else groups.push({ chunk: piece.chunk, texts: [piece.text] });
  }
  // Names run together — "Claude Max" is one phrase — while values, which
  // would otherwise blur into each other, keep their separator.
  return groups
    .map((group) => group.texts.join(group.chunk === 'identity' ? ' ' : ' · '))
    .join(CHUNK_GAP);
}

export function formatStatusText(
  snapshot: UsageSnapshot | undefined,
  nowMs: number,
  options: FormatOptions,
  /** Window rendered by the alert item, and so left out of this one. */
  alerted?: UsageLimit,
): string {
  if (!snapshot) return EMPTY_TEXT;
  if (!snapshot.available) return '$(pulse) Claude usage n/a';

  const label = options.label.trim();
  const pieces: Piece[] = label ? [{ chunk: 'identity', text: label }] : [];

  for (const segment of options.segments) {
    const text = segmentText(segment, snapshot, nowMs, alerted);
    if (text) pieces.push({ chunk: SEGMENT_CHUNK[segment], text });
  }

  // Nothing but the label left: either the alert item is carrying the whole
  // reading, or there is no reading and the placeholder should name itself.
  if (pieces.length === (label ? 1 : 0)) {
    return alerted ? `$(pulse) ${label}`.trimEnd() : EMPTY_TEXT;
  }

  const stale = isStale(snapshot, nowMs, options.staleAfterMs) ? ' (stale)' : '';
  return `$(pulse) ${joinChunks(pieces)}${stale}`;
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
  if (snapshot.subscriptionType) details.push(`- Plan: ${formatPlan(snapshot.subscriptionType)}`);
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
