import type { RateLimitWindow, StatusLinePayload, UsageSnapshot, UsageWindow } from './types';

/** Below this, a numeric timestamp is seconds rather than milliseconds. */
const MS_THRESHOLD = 1e11;

/**
 * Claude Code passes `resets_at` straight through from the API without
 * normalising it, so accept both ISO-8601 strings and epoch numbers (in either
 * seconds or milliseconds, whether typed as a number or a numeric string).
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

function parseWindow(raw: RateLimitWindow | undefined): UsageWindow | undefined {
  if (!raw || typeof raw !== 'object') return undefined;

  const percent = typeof raw.used_percentage === 'number' ? raw.used_percentage : undefined;
  const resetsAtMs = parseResetsAt(raw.resets_at);
  if (percent === undefined || !Number.isFinite(percent)) {
    // A window with only a reset time is not worth a segment, but keep it if
    // that is all Claude Code sent so the countdown still renders.
    return resetsAtMs === undefined ? undefined : { usedPercent: 0, resetsAtMs };
  }
  // Overage plans can report past 100; only the floor is nonsense.
  return { usedPercent: Math.max(0, percent), resetsAtMs };
}

function finiteNumber(value: unknown): number | undefined {
  return typeof value === 'number' && Number.isFinite(value) ? value : undefined;
}

function nonEmptyString(value: unknown): string | undefined {
  return typeof value === 'string' && value.trim() ? value : undefined;
}

/** Extract the fields the status bar needs. Returns undefined for junk input. */
export function toSnapshot(payload: unknown, receivedAtMs: number): UsageSnapshot | undefined {
  if (typeof payload !== 'object' || payload === null || Array.isArray(payload)) return undefined;
  const p = payload as StatusLinePayload;

  return {
    receivedAtMs,
    sessionId: nonEmptyString(p.session_id),
    sessionName: nonEmptyString(p.session_name),
    modelName: nonEmptyString(p.model?.display_name) ?? nonEmptyString(p.model?.id),
    fiveHour: parseWindow(p.rate_limits?.five_hour),
    sevenDay: parseWindow(p.rate_limits?.seven_day),
    costUsd: finiteNumber(p.cost?.total_cost_usd),
    contextUsedPercent: finiteNumber(p.context_window?.used_percentage),
    contextWindowSize: finiteNumber(p.context_window?.context_window_size),
    claudeVersion: nonEmptyString(p.version),
  };
}

/** Parse the payload file. Returns undefined when it is absent or malformed. */
export function parseSnapshot(text: string, receivedAtMs: number): UsageSnapshot | undefined {
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    return undefined;
  }
  return toSnapshot(parsed, receivedAtMs);
}

/** True when the payload carries plan usage — the reason this extension exists. */
export function hasRateLimits(snapshot: UsageSnapshot | undefined): boolean {
  return Boolean(snapshot && (snapshot.fiveHour || snapshot.sevenDay));
}
