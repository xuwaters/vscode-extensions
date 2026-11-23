import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { CoalescedRefresh, type RefreshRun } from './coalesce.js';

/** A run whose end the test decides, recording whether it was cancelled. */
interface ControlledRun extends RefreshRun {
  cancelled: boolean;
  finish(): void;
}

function controlledRuns() {
  const runs: ControlledRun[] = [];
  const start = (): RefreshRun => {
    let finish!: () => void;
    const done = new Promise<void>((resolve) => (finish = resolve));
    const run: ControlledRun = {
      done,
      cancelled: false,
      cancel() {
        run.cancelled = true;
      },
      finish,
    };
    runs.push(run);
    return run;
  };
  return { runs, start };
}

describe('CoalescedRefresh', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('folds a burst of requests into one run', () => {
    const { runs, start } = controlledRuns();
    const refresh = new CoalescedRefresh(300, start);

    for (let i = 0; i < 229; i++) refresh.schedule();
    expect(runs).toHaveLength(0);

    vi.advanceTimersByTime(300);
    expect(runs).toHaveLength(1);
  });

  it('keeps a single run live when requests arrive during it', async () => {
    const { runs, start } = controlledRuns();
    const refresh = new CoalescedRefresh(300, start);

    refresh.schedule();
    vi.advanceTimersByTime(300);
    expect(runs).toHaveLength(1);

    // Three separate bursts while the first run is still walking.
    for (let i = 0; i < 3; i++) {
      refresh.schedule();
      vi.advanceTimersByTime(300);
    }
    expect(runs).toHaveLength(1);
    expect(runs[0].cancelled).toBe(true);

    runs[0].finish();
    await vi.runAllTimersAsync();
    expect(runs).toHaveLength(2);
    expect(runs[1].cancelled).toBe(false);

    runs[1].finish();
    await vi.runAllTimersAsync();
    expect(runs).toHaveLength(2);
  });

  it('starts the queued run even when the cancelled one rejects', async () => {
    const runs: RefreshRun[] = [];
    let reject!: (error: Error) => void;
    const refresh = new CoalescedRefresh(10, () => {
      const run: RefreshRun = {
        done: runs.length === 0
          ? new Promise((_, fail) => (reject = fail))
          : Promise.resolve(),
        cancel: () => reject(new Error('Canceled')),
      };
      runs.push(run);
      return run;
    });

    refresh.schedule();
    vi.advanceTimersByTime(10);
    refresh.schedule();
    vi.advanceTimersByTime(10);

    await vi.runAllTimersAsync();
    expect(runs).toHaveLength(2);
  });

  it('cancels the live run and starts nothing more once disposed', async () => {
    const { runs, start } = controlledRuns();
    const refresh = new CoalescedRefresh(300, start);

    refresh.schedule();
    vi.advanceTimersByTime(300);
    refresh.schedule();
    vi.advanceTimersByTime(300);

    refresh.dispose();
    expect(runs[0].cancelled).toBe(true);

    runs[0].finish();
    refresh.schedule();
    await vi.runAllTimersAsync();
    expect(runs).toHaveLength(1);
  });
});
