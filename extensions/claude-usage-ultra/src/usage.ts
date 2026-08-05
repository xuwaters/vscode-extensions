/**
 * Shape of the `get_usage` control response, and the normalised view of it the
 * status bar renders.
 *
 * Claude Code answers a `{subtype: "get_usage"}` control request with the plan
 * utilisation it fetched from `/api/oauth/usage`, reshaped a little. Only the
 * fields consumed here are typed; the response also carries `session` (always
 * zeroes for the throwaway process we spawn) and `behaviors` (usage analytics),
 * neither of which belongs in a status bar.
 */

/** Below this, a numeric timestamp is seconds rather than milliseconds. */
const MS_THRESHOLD = 1e11;

export interface RawUsageResponse {
  subscription_type?: string | null;
  rate_limits_available?: boolean;
  rate_limits?: RawRateLimits | null;
}

export interface RawRateLimits {
  /**
   * Pre-grouped windows, newest shape. Every entry the account has is here,
   * including per-model ones, so this is preferred over the sibling keys.
   */
  limits?: RawLimit[] | null;
  /** Older per-window keys, kept as a fallback for CLIs without `limits`. */
  five_hour?: RawWindow | null;
  seven_day?: RawWindow | null;
  seven_day_opus?: RawWindow | null;
  seven_day_sonnet?: RawWindow | null;
  seven_day_oauth_apps?: RawWindow | null;
  spend?: RawSpend | null;
  extra_usage?: RawExtraUsage | null;
}

export interface RawLimit {
  /** `session`, `weekly_all`, `weekly_scoped`, ... */
  kind?: string | null;
  /** `session` or `weekly`. */
  group?: string | null;
  /** Already 0..100. */
  percent?: number | null;
  severity?: string | null;
  resets_at?: string | number | null;
  scope?: { model?: { id?: string | null; display_name?: string | null } | null } | null;
  /** True for the window currently constraining the account. */
  is_active?: boolean | null;
}

export interface RawWindow {
  /** Also 0..100 — the CLI has already scaled the API's 0..1 fraction. */
  utilization?: number | null;
  resets_at?: string | number | null;
}

/** Money as minor units plus the exponent to place the decimal point. */
export interface RawMoney {
  amount_minor?: number | null;
  currency?: string | null;
  exponent?: number | null;
}

export interface RawSpend {
  used?: RawMoney | null;
  limit?: RawMoney | null;
  percent?: number | null;
  severity?: string | null;
  enabled?: boolean | null;
  disabled_reason?: string | null;
}

export interface RawExtraUsage {
  is_enabled?: boolean | null;
  monthly_limit?: number | null;
  used_credits?: number | null;
  utilization?: number | null;
  currency?: string | null;
  decimal_places?: number | null;
  disabled_reason?: string | null;
}

export type Severity = 'normal' | 'warning' | 'critical';

export interface UsageLimit {
  /** Raw kind, so callers can pick a window without matching on the label. */
  kind: string;
  /** `session` or `weekly`; unknown kinds fall back to `other`. */
  group: 'session' | 'weekly' | 'other';
  /** Ready to render: "Session (5h)", "Weekly · Opus 4.6". */
  label: string;
  /** Model display name for a scoped window, if there is one. */
  scopeName?: string;
  percent: number;
  severity: Severity;
  resetsAtMs?: number;
  /** The window currently constraining the account. */
  isActive: boolean;
}

export interface UsageSpend {
  usedUsd: number;
  limitUsd: number;
  currency: string;
  percent: number;
  enabled: boolean;
  disabledReason?: string;
}

export interface UsageSnapshot {
  /** Epoch milliseconds this reading was taken. */
  fetchedAtMs: number;
  /** False when the login method has no plan limits (API key, Bedrock, ...). */
  available: boolean;
  subscriptionType?: string;
  limits: UsageLimit[];
  spend?: UsageSpend;
}

/**
 * Accept ISO-8601 strings and epoch numbers in either seconds or milliseconds.
 * Claude Code sends ISO here, but the same field is epoch seconds elsewhere in
 * its protocol, so do not assume.
 */
export function parseResetsAt(value: unknown): number | undefined {
  if (typeof value === 'number') {
    if (!Number.isFinite(value) || value <= 0) return undefined;
    return value > MS_THRESHOLD ? value : value * 1000;
  }
  if (typeof value !== 'string') return undefined;

  const trimmed = value.trim();
  if (!trimmed) return undefined;
  if (/^\d+(\.\d+)?$/.test(trimmed)) return parseResetsAt(Number(trimmed));

  const parsed = Date.parse(trimmed);
  return Number.isNaN(parsed) ? undefined : parsed;
}

function finiteNumber(value: unknown): number | undefined {
  return typeof value === 'number' && Number.isFinite(value) ? value : undefined;
}

function nonEmptyString(value: unknown): string | undefined {
  return typeof value === 'string' && value.trim() ? value : undefined;
}

function parseSeverity(value: unknown): Severity {
  return value === 'critical' || value === 'warning' ? value : 'normal';
}

function parseGroup(value: unknown, kind: string): UsageLimit['group'] {
  if (value === 'session' || value === 'weekly') return value;
  if (kind === 'session' || kind === 'five_hour') return 'session';
  if (kind.startsWith('weekly') || kind.startsWith('seven_day')) return 'weekly';
  return 'other';
}

/** "seven_day_oauth_apps" -> "Seven day oauth apps", for kinds we do not know. */
function humanise(kind: string): string {
  const spaced = kind.replace(/[_-]+/g, ' ').trim();
  return spaced ? spaced.charAt(0).toUpperCase() + spaced.slice(1) : 'Limit';
}

const KIND_LABELS: Record<string, string> = {
  session: 'Session (5h)',
  five_hour: 'Session (5h)',
  weekly_all: 'Weekly, all models',
  seven_day: 'Weekly, all models',
  seven_day_opus: 'Weekly, Opus',
  seven_day_sonnet: 'Weekly, Sonnet',
  seven_day_oauth_apps: 'Weekly, OAuth apps',
};

export function labelFor(kind: string, scopeName?: string): string {
  if (scopeName) {
    const prefix = kind.startsWith('session') ? 'Session' : 'Weekly';
    return `${prefix} · ${scopeName}`;
  }
  return KIND_LABELS[kind] ?? humanise(kind);
}

function parseLimit(raw: unknown): UsageLimit | undefined {
  if (typeof raw !== 'object' || raw === null) return undefined;
  const entry = raw as RawLimit;

  const percent = finiteNumber(entry.percent);
  if (percent === undefined) return undefined;

  const kind = nonEmptyString(entry.kind) ?? 'limit';
  const scopeName = nonEmptyString(entry.scope?.model?.display_name);

  return {
    kind,
    group: parseGroup(entry.group, kind),
    label: labelFor(kind, scopeName),
    scopeName,
    // Overage plans report past 100; only the floor is nonsense.
    percent: Math.max(0, percent),
    severity: parseSeverity(entry.severity),
    resetsAtMs: parseResetsAt(entry.resets_at),
    isActive: entry.is_active === true,
  };
}

/** The pre-`limits` shape: one key per window, `utilization` already 0..100. */
const FALLBACK_WINDOWS: Array<keyof RawRateLimits> = [
  'five_hour',
  'seven_day',
  'seven_day_opus',
  'seven_day_sonnet',
  'seven_day_oauth_apps',
];

function parseFallbackWindows(rateLimits: RawRateLimits): UsageLimit[] {
  const limits: UsageLimit[] = [];

  for (const key of FALLBACK_WINDOWS) {
    const window = rateLimits[key];
    if (typeof window !== 'object' || window === null) continue;

    const percent = finiteNumber((window as RawWindow).utilization);
    if (percent === undefined) continue;

    const kind = String(key);
    limits.push({
      kind,
      group: parseGroup(undefined, kind),
      label: labelFor(kind),
      percent: Math.max(0, percent),
      severity: 'normal',
      resetsAtMs: parseResetsAt((window as RawWindow).resets_at),
      isActive: false,
    });
  }

  return limits;
}

function minorToMajor(money: RawMoney | null | undefined): number | undefined {
  const amount = finiteNumber(money?.amount_minor);
  if (amount === undefined) return undefined;
  const exponent = finiteNumber(money?.exponent) ?? 2;
  return amount / 10 ** exponent;
}

function parseSpend(rateLimits: RawRateLimits): UsageSpend | undefined {
  const spend = rateLimits.spend;
  if (spend && typeof spend === 'object') {
    const usedUsd = minorToMajor(spend.used);
    const limitUsd = minorToMajor(spend.limit);
    if (usedUsd !== undefined && limitUsd !== undefined) {
      return {
        usedUsd,
        limitUsd,
        currency: nonEmptyString(spend.used?.currency) ?? 'USD',
        percent: finiteNumber(spend.percent) ?? (limitUsd > 0 ? (usedUsd / limitUsd) * 100 : 0),
        enabled: spend.enabled === true,
        disabledReason: nonEmptyString(spend.disabled_reason),
      };
    }
  }

  // `extra_usage` carries the same numbers in credits — minor units under a
  // different name — for CLIs that predate the `spend` block.
  const extra = rateLimits.extra_usage;
  if (!extra || typeof extra !== 'object') return undefined;

  const used = finiteNumber(extra.used_credits);
  const limit = finiteNumber(extra.monthly_limit);
  if (used === undefined || limit === undefined) return undefined;

  const scale = 10 ** (finiteNumber(extra.decimal_places) ?? 2);
  const usedUsd = used / scale;
  const limitUsd = limit / scale;

  return {
    usedUsd,
    limitUsd,
    currency: nonEmptyString(extra.currency) ?? 'USD',
    percent: finiteNumber(extra.utilization) ?? (limitUsd > 0 ? (usedUsd / limitUsd) * 100 : 0),
    enabled: extra.is_enabled === true,
    disabledReason: nonEmptyString(extra.disabled_reason),
  };
}

/** Session first, then weekly; within a group, the fullest window leads. */
function orderLimits(limits: UsageLimit[]): UsageLimit[] {
  const rank: Record<UsageLimit['group'], number> = { session: 0, weekly: 1, other: 2 };
  return [...limits].sort((a, b) => rank[a.group] - rank[b.group] || b.percent - a.percent);
}

/** Normalise a `get_usage` response. Returns undefined for junk input. */
export function toSnapshot(response: unknown, fetchedAtMs: number): UsageSnapshot | undefined {
  if (typeof response !== 'object' || response === null || Array.isArray(response)) {
    return undefined;
  }
  const raw = response as RawUsageResponse;
  const rateLimits =
    typeof raw.rate_limits === 'object' && raw.rate_limits !== null ? raw.rate_limits : undefined;

  const fromList = Array.isArray(rateLimits?.limits)
    ? rateLimits.limits.map(parseLimit).filter((l): l is UsageLimit => l !== undefined)
    : [];
  const limits = fromList.length > 0 ? fromList : rateLimits ? parseFallbackWindows(rateLimits) : [];

  return {
    fetchedAtMs,
    // Treat a missing flag as available so a response carrying limits is not
    // discarded by a CLI that stops sending it.
    available: raw.rate_limits_available !== false,
    subscriptionType: nonEmptyString(raw.subscription_type),
    limits: orderLimits(limits),
    spend: rateLimits ? parseSpend(rateLimits) : undefined,
  };
}

/** The session (5h) window, if the account has one. */
export function sessionLimit(snapshot: UsageSnapshot): UsageLimit | undefined {
  return snapshot.limits.find((limit) => limit.group === 'session');
}

/** The all-models weekly window, which is the one most people watch. */
export function weeklyLimit(snapshot: UsageSnapshot): UsageLimit | undefined {
  return (
    snapshot.limits.find((limit) => limit.kind === 'weekly_all' || limit.kind === 'seven_day') ??
    snapshot.limits.find((limit) => limit.group === 'weekly' && !limit.scopeName)
  );
}

/** Per-model weekly windows, fullest first. */
export function scopedLimits(snapshot: UsageSnapshot): UsageLimit[] {
  return snapshot.limits.filter((limit) => limit.scopeName !== undefined);
}
