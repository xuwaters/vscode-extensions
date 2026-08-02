import { afterEach, describe, expect, it, vi } from 'vitest';
import { ParkedScroll, SyncGuard } from './scrollSync';

describe('SyncGuard', () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it('starts unsuppressed', () => {
    expect(new SyncGuard().suppressed).toBe(false);
  });

  it('suppresses for its window and then lets events through', () => {
    vi.useFakeTimers();
    const guard = new SyncGuard(150);
    guard.suppress();
    expect(guard.suppressed).toBe(true);
    vi.advanceTimersByTime(149);
    expect(guard.suppressed).toBe(true);
    vi.advanceTimersByTime(1);
    expect(guard.suppressed).toBe(false);
  });

  it('extends the window on a fresh suppression', () => {
    vi.useFakeTimers();
    const guard = new SyncGuard(150);
    guard.suppress();
    vi.advanceTimersByTime(100);
    guard.suppress();
    vi.advanceTimersByTime(100);
    expect(guard.suppressed).toBe(true);
  });
});

describe('ParkedScroll', () => {
  it('has nothing to claim until a line is parked', () => {
    const parked = new ParkedScroll();
    expect(parked.pending).toBe(false);
    expect(parked.claim()).toBeUndefined();
  });

  it('hands the parked line back once', () => {
    const parked = new ParkedScroll();
    parked.park(42);
    expect(parked.pending).toBe(true);
    expect(parked.claim()).toBe(42);
    expect(parked.pending).toBe(false);
    expect(parked.claim()).toBeUndefined();
  });

  it('keeps only the latest position', () => {
    const parked = new ParkedScroll();
    parked.park(10);
    parked.park(20);
    expect(parked.claim()).toBe(20);
  });

  it('parks line 0 as a real position', () => {
    const parked = new ParkedScroll();
    parked.park(0);
    expect(parked.pending).toBe(true);
    expect(parked.claim()).toBe(0);
  });

  it('drops the parked line when the side is already in place', () => {
    const parked = new ParkedScroll();
    parked.park(7);
    parked.clear();
    expect(parked.pending).toBe(false);
    expect(parked.claim()).toBeUndefined();
  });
});
