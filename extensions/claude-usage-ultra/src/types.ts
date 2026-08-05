/**
 * Shape of the JSON Claude Code pipes to a `statusLine` command on stdin.
 *
 * Only the fields this extension consumes are typed; the payload carries more
 * (workspace, vim, pr, worktree, ...). Everything is optional because Claude
 * Code omits blocks that do not apply — notably `rate_limits`, which is absent
 * until the CLI has fetched plan utilisation at least once.
 */
export interface StatusLinePayload {
  session_id?: string;
  session_name?: string;
  transcript_path?: string;
  cwd?: string;
  version?: string;
  model?: { id?: string; display_name?: string };
  workspace?: { current_dir?: string; project_dir?: string };
  cost?: {
    total_cost_usd?: number;
    total_duration_ms?: number;
    total_api_duration_ms?: number;
    total_lines_added?: number;
    total_lines_removed?: number;
  };
  context_window?: {
    total_input_tokens?: number;
    total_output_tokens?: number;
    context_window_size?: number;
    used_percentage?: number;
    remaining_percentage?: number;
  };
  exceeds_200k_tokens?: boolean;
  rate_limits?: {
    /** Rolling 5-hour session window. */
    five_hour?: RateLimitWindow;
    /** Rolling 7-day window across all models. */
    seven_day?: RateLimitWindow;
  };
}

export interface RateLimitWindow {
  /** Already scaled to 0..100 by Claude Code (the API reports 0..1). */
  used_percentage?: number;
  resets_at?: string | number;
}

/** A usage window normalised for display. */
export interface UsageWindow {
  usedPercent: number;
  /** Epoch milliseconds, when Claude Code reported a reset time. */
  resetsAtMs?: number;
}

/** Everything the status bar needs, extracted from one payload. */
export interface UsageSnapshot {
  /** Epoch milliseconds the payload was written. */
  receivedAtMs: number;
  sessionId?: string;
  sessionName?: string;
  modelName?: string;
  fiveHour?: UsageWindow;
  sevenDay?: UsageWindow;
  costUsd?: number;
  contextUsedPercent?: number;
  contextWindowSize?: number;
  claudeVersion?: string;
}
