import * as fs from 'node:fs';
import * as path from 'node:path';
import { parseSnapshot } from './payload';
import type { UsageSnapshot } from './types';

export const PAYLOAD_FILE = 'current.json';

/** Coalesce the burst of events a rename produces. */
const DEBOUNCE_MS = 150;

/**
 * Watches the file the bridge script writes.
 *
 * The script writes to a temp file and renames it into place, so watching the
 * directory (rather than the file) survives the inode swap. A poll is kept as a
 * backstop because `fs.watch` is unreliable on network and virtualised volumes.
 */
export class PayloadWatcher {
  private watcher: fs.FSWatcher | undefined;
  private pollTimer: ReturnType<typeof setInterval> | undefined;
  private debounceTimer: ReturnType<typeof setTimeout> | undefined;
  private lastMtimeMs = -1;
  private current: UsageSnapshot | undefined;

  constructor(
    private readonly stateDir: string,
    private readonly onChange: (snapshot: UsageSnapshot | undefined) => void,
  ) {}

  get snapshot(): UsageSnapshot | undefined {
    return this.current;
  }

  get payloadPath(): string {
    return path.join(this.stateDir, PAYLOAD_FILE);
  }

  start(pollIntervalMs: number): void {
    try {
      fs.mkdirSync(this.stateDir, { recursive: true });
    } catch {
      // Directory creation is best effort; the poll below will keep retrying.
    }

    try {
      this.watcher = fs.watch(this.stateDir, { persistent: false }, (_event, filename) => {
        if (filename && path.basename(filename) !== PAYLOAD_FILE) return;
        if (this.debounceTimer) clearTimeout(this.debounceTimer);
        this.debounceTimer = setTimeout(() => this.refresh(), DEBOUNCE_MS);
      });
    } catch {
      this.watcher = undefined;
    }

    this.pollTimer = setInterval(() => this.refresh(), pollIntervalMs);
    this.refresh(true);
  }

  setPollInterval(pollIntervalMs: number): void {
    if (this.pollTimer) clearInterval(this.pollTimer);
    this.pollTimer = setInterval(() => this.refresh(), pollIntervalMs);
  }

  /** Re-read the payload; notifies only when the file actually changed. */
  refresh(force = false): void {
    let stat: fs.Stats;
    try {
      stat = fs.statSync(this.payloadPath);
    } catch {
      if (this.current !== undefined || force) {
        this.current = undefined;
        this.lastMtimeMs = -1;
        this.onChange(undefined);
      }
      return;
    }

    if (!force && stat.mtimeMs === this.lastMtimeMs) return;
    this.lastMtimeMs = stat.mtimeMs;

    let text: string;
    try {
      text = fs.readFileSync(this.payloadPath, 'utf8');
    } catch {
      return;
    }

    const next = parseSnapshot(text, stat.mtimeMs);
    // A malformed write is transient; keep showing the last good reading.
    if (!next) return;

    this.current = next;
    this.onChange(next);
  }

  dispose(): void {
    if (this.debounceTimer) clearTimeout(this.debounceTimer);
    if (this.pollTimer) clearInterval(this.pollTimer);
    this.watcher?.close();
    this.watcher = undefined;
    this.pollTimer = undefined;
    this.debounceTimer = undefined;
  }
}
