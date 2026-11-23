/** One run of the refresh, and the way to abandon it. */
export interface RefreshRun {
  readonly done: Promise<unknown>;
  cancel(): void;
}

/**
 * A refresh that may be asked for far more often than it can be afforded.
 *
 * Requests are debounced, and at most one run is ever live: a request that
 * lands while one is running cancels it and queues exactly one more, started
 * once the cancelled run has settled. A burst of a few hundred file events —
 * a branch switch, a test writing fixtures — therefore costs one or two runs,
 * not one per event.
 */
export class CoalescedRefresh {
  private timer: ReturnType<typeof setTimeout> | undefined;
  private current: RefreshRun | undefined;
  private again = false;
  private disposed = false;

  constructor(
    private readonly delayMs: number,
    private readonly start: () => RefreshRun,
  ) {}

  /** Ask for a refresh `delayMs` from now, folding in any already asked for. */
  schedule(): void {
    if (this.disposed) return;
    if (this.timer) clearTimeout(this.timer);
    this.timer = setTimeout(() => {
      this.timer = undefined;
      this.fire();
    }, this.delayMs);
  }

  /** Drop any pending request and cancel the run in flight. */
  dispose(): void {
    this.disposed = true;
    if (this.timer) clearTimeout(this.timer);
    this.timer = undefined;
    this.again = false;
    this.current?.cancel();
  }

  private fire(): void {
    if (this.current) {
      // What it is computing is already stale. Let it die, then start over.
      this.again = true;
      this.current.cancel();
      return;
    }

    const run = this.start();
    this.current = run;
    void run.done
      .catch(() => undefined)
      .finally(() => {
        this.current = undefined;
        if (this.again && !this.disposed) {
          this.again = false;
          this.fire();
        }
      });
  }
}
