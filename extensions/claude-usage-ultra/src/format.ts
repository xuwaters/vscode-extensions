import {
  scopedLimits,
  sessionLimit,
  weeklyLimit,
  type UsageLimit,
  type UsageSnapshot,
  type UsageSpend,
} from './usage';

export type Segment = 'session' | 'weekly' | 'scoped' | 'reset' | 'spend' | 'plan';

/** How full one window is, in the four steps the dots distinguish. */
export type Level = 'normal' | 'notice' | 'warning' | 'critical';

/**
 * A status bar item paints all of its text one colour, so per-window colour has
 * to come from the characters themselves — and emoji are the only glyphs that
 * carry their own. Every window gets a dot, green included: a row of dots that
 * all read the same width is easier to scan than one with holes in it, and a
 * green dot says "checked, fine" where a blank says nothing at all.
 */
const DOTS: Record<Level, string> = {
  normal: '🟢',
  notice: '🟡',
  warning: '🟠',
  critical: '🔴',
};

export interface FormatOptions {
  segments: Segment[];
  /** Prefix naming the item, so the numbers are not anonymous. Empty hides it. */
  label: string;
  staleAfterMs: number;
  noticeAtPercent: number;
  warnAtPercent: number;
  criticalAtPercent: number;
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

/** "$" for dollars; anything else names itself, since its symbol may not render. */
function currencyPrefix(currency: string): string {
  return currency === 'USD' ? '$' : `${currency} `;
}

function formatMoney(amount: number, currency: string): string {
  return `${currencyPrefix(currency)}${amount.toFixed(2)}`;
}

/** Cents when there are cents to show, nothing when there are not: "$4000". */
function formatAmount(amount: number): string {
  const cents = Math.round(amount * 100);
  const rounded = cents / 100;
  return cents % 100 === 0 ? String(rounded) : rounded.toFixed(2);
}

/**
 * "$72.77/$200". A spend on its own is a number without a scale — $72 is
 * nothing against a $4000 cap and most of the way through a $100 one — so the
 * cap travels with it. The cap is nearly always round, and its ".00" is width
 * the bar cannot spare.
 */
function formatSpend(spend: UsageSpend): string {
  const prefix = currencyPrefix(spend.currency);
  const used = `${prefix}${formatAmount(spend.usedUsd)}`;
  // An uncapped account has nothing to compare against.
  if (!(spend.limitUsd > 0)) return used;
  // A symbol repeats cheaply; a currency code would double the width to say
  // what the first number already said.
  const capPrefix = prefix.endsWith(' ') ? '' : prefix;
  return `${used} / ${capPrefix}${formatAmount(spend.limitUsd)}`;
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

/**
 * How full one window is. Claude Code's own severity is trusted when it
 * escalates, then the user's thresholds take over so the dots still track a
 * plain percentage.
 */
export function levelFor(limit: UsageLimit, options: FormatOptions): Level {
  if (limit.severity === 'critical' || limit.percent >= options.criticalAtPercent) return 'critical';
  if (limit.severity === 'warning' || limit.percent >= options.warnAtPercent) return 'warning';
  if (limit.percent >= options.noticeAtPercent) return 'notice';
  return 'normal';
}

/** How a window names itself in the bar: "5h", "7d", "Fable". */
function shortLabel(limit: UsageLimit): string {
  if (limit.scopeName) return limit.scopeName;
  if (limit.group === 'session') return '5h';
  if (limit.group === 'weekly') return '7d';
  return limit.label;
}

/** "🟢 5h 7%", "🟠 7d 84%" — the dot leads, so a scan reads the colours first. */
function windowText(limit: UsageLimit, options: FormatOptions): string {
  return `${DOTS[levelFor(limit, options)]} ${shortLabel(limit)} ${formatPercent(limit.percent)}`;
}

function segmentText(
  segment: Segment,
  snapshot: UsageSnapshot,
  nowMs: number,
  options: FormatOptions,
): string | undefined {
  switch (segment) {
    case 'session': {
      const limit = sessionLimit(snapshot);
      return limit ? windowText(limit, options) : undefined;
    }
    case 'weekly': {
      const limit = weeklyLimit(snapshot);
      return limit ? windowText(limit, options) : undefined;
    }
    case 'scoped': {
      const limit = topScoped(snapshot);
      return limit ? windowText(limit, options) : undefined;
    }
    case 'reset': {
      const limit = nextReset(snapshot);
      return limit ? `$(history) ${formatCountdown(limit.resetsAtMs! - nowMs)}` : undefined;
    }
    case 'spend':
      // The card icon is what marks this as money rather than one more percentage.
      return snapshot.spend ? `$(credit-card) ${formatSpend(snapshot.spend)}` : undefined;
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
): string {
  if (!snapshot) return EMPTY_TEXT;
  if (!snapshot.available) return '$(pulse) Claude usage n/a';

  const label = options.label.trim();
  const pieces: Piece[] = label ? [{ chunk: 'identity', text: label }] : [];

  for (const segment of options.segments) {
    const text = segmentText(segment, snapshot, nowMs, options);
    if (text) pieces.push({ chunk: SEGMENT_CHUNK[segment], text });
  }

  // The placeholder already names itself, so an empty reading needs no label.
  if (pieces.length === (label ? 1 : 0)) return EMPTY_TEXT;

  const stale = isStale(snapshot, nowMs, options.staleAfterMs) ? ' (stale)' : '';
  return `$(pulse) ${joinChunks(pieces)}${stale}`;
}

function timeAgo(ms: number): string {
  if (ms < MINUTE) return 'just now';
  return `${formatCountdown(ms)} ago`;
}

function limitLine(limit: UsageLimit, nowMs: number, options: FormatOptions): string {
  const reset =
    limit.resetsAtMs === undefined
      ? ''
      : ` · resets in ${formatCountdown(limit.resetsAtMs - nowMs)}`;
  const active = limit.isActive ? ' — currently limiting' : '';
  // The same dot as the bar, so the hover confirms what the glance suggested.
  const dot = DOTS[levelFor(limit, options)];
  return `- ${dot} ${limit.label}: **${formatPercent(limit.percent)}**${reset}${active}`;
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
    for (const limit of snapshot.limits) lines.push(limitLine(limit, nowMs, options));
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
