import { describe, expect, it, vi } from 'vitest';
import { SyncGuard } from './sync.js';

describe('the two-way sync loop guard', () => {
  it('suppresses a reply within the window and allows one after it', () => {
    let now = 1000;
    const guard = new SyncGuard(() => now);

    guard.markEditorOrigin();
    expect(guard.isEditorOrigin()).toBe(true);

    now += SyncGuard.WINDOW_MS - 1;
    expect(guard.isEditorOrigin()).toBe(true);

    now += 2;
    expect(guard.isEditorOrigin()).toBe(false);
  });

  it('tracks the two directions independently', () => {
    let now = 0;
    const guard = new SyncGuard(() => now);

    guard.markPreviewOrigin();
    expect(guard.isPreviewOrigin()).toBe(true);
    expect(guard.isEditorOrigin()).toBe(false);
  });

  it('starts with neither side suppressed', () => {
    const guard = new SyncGuard(() => 10_000);
    expect(guard.isEditorOrigin()).toBe(false);
    expect(guard.isPreviewOrigin()).toBe(false);
  });

  it('collapses repeated debounced calls into one', () => {
    vi.useFakeTimers();
    try {
      const guard = new SyncGuard();
      const action = vi.fn();

      guard.debounce(50, action);
      vi.advanceTimersByTime(40);
      guard.debounce(50, action);
      vi.advanceTimersByTime(40);
      expect(action).not.toHaveBeenCalled();

      vi.advanceTimersByTime(20);
      expect(action).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it('cancels a pending debounced call', () => {
    vi.useFakeTimers();
    try {
      const guard = new SyncGuard();
      const action = vi.fn();

      guard.debounce(50, action);
      guard.cancel();
      vi.advanceTimersByTime(200);

      expect(action).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });
});
