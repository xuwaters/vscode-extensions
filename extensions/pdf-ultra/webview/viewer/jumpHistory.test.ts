// @vitest-environment happy-dom
// (Nothing here touches the DOM, but `@microsoft/fast-element`'s entry point
// reaches for `document` as it loads, and observability comes from there.)
import { Observable } from '@microsoft/fast-element';
import { describe, expect, it } from 'vitest';
import { JumpHistory } from './jumpHistory.js';

const at = (page: number, offsetRatio = 0): { page: number; offsetRatio: number } => ({
  page,
  offsetRatio,
});

describe('the jump history', () => {
  it('has nowhere to go back to before anything has jumped', () => {
    expect(new JumpHistory().canGoBack).toBe(false);
    expect(new JumpHistory().pop()).toBeUndefined();
  });

  it('gives the jumps back in the order they were made, most recent first', () => {
    const history = new JumpHistory();
    history.push(at(3, 0.25));
    history.push(at(20));
    expect(history.canGoBack).toBe(true);
    expect(history.pop()).toEqual(at(20));
    expect(history.pop()).toEqual(at(3, 0.25));
    expect(history.canGoBack).toBe(false);
  });

  /** A link that lands where the reader already stands is not a place to leave. */
  it('does not stack the same place twice', () => {
    const history = new JumpHistory();
    history.push(at(7, 0.5));
    history.push(at(7, 0.502));
    expect(history.depth).toBe(1);
    history.push(at(7, 0.9));
    expect(history.depth).toBe(2);
  });

  it('forgets the oldest jumps rather than growing without end', () => {
    const history = new JumpHistory();
    for (let page = 1; page <= 120; page += 1) history.push(at(page));
    expect(history.depth).toBe(50);
    expect(history.pop()).toEqual(at(120));
  });

  it('drops everything when a different document is opened', () => {
    const history = new JumpHistory();
    history.push(at(4));
    history.clear();
    expect(history.canGoBack).toBe(false);
  });

  /**
   * The toolbar's disabled binding reads `depth`, so both ends of the stack
   * have to notify — a button that stays greyed out after the first jump is
   * the same bug as one that never re-enables.
   */
  it('notifies when the button should change', () => {
    const history = new JumpHistory();
    let notifications = 0;
    Observable.getNotifier(history).subscribe(
      { handleChange: () => (notifications += 1) },
      'depth',
    );
    history.push(at(2));
    expect(notifications).toBe(1);
    history.pop();
    expect(notifications).toBe(2);
  });
});
