// @vitest-environment happy-dom
// (Nothing here touches the DOM, but `@microsoft/fast-element`'s entry point
// reaches for `document` as it loads, and observability comes from there.)
import { Observable } from '@microsoft/fast-element';
import { describe, expect, it } from 'vitest';
import type { RawOutlineItem } from '../model/outline.js';
import { OutlineState } from './outlineState.js';

const tree: RawOutlineItem[] = [
  { title: 'One', items: [{ title: 'One.a' }, { title: 'One.b' }] },
  { title: 'Two' },
];

/** Count how many times a binding over `property` would be re-evaluated. */
function watch(source: object, property: string): () => number {
  let notifications = 0;
  Observable.getNotifier(source).subscribe(
    { handleChange: () => (notifications += 1) },
    property,
  );
  return () => notifications;
}

describe('the outline sidebar state', () => {
  it('shows the whole tree when a document has one', () => {
    const outline = new OutlineState();
    outline.load(tree);
    expect(outline.shown.map((row) => row.title)).toEqual(['One', 'One.a', 'One.b', 'Two']);
  });

  it('shows nothing for a document without one', () => {
    const outline = new OutlineState();
    outline.load(null);
    expect(outline.shown).toEqual([]);
  });

  it('hides a subtree when its row is collapsed, and brings it back', () => {
    const outline = new OutlineState();
    outline.load(tree);
    const [first] = outline.all;
    outline.toggle(first!);
    expect(outline.shown.map((row) => row.title)).toEqual(['One', 'Two']);
    outline.toggle(first!);
    expect(outline.shown).toHaveLength(4);
  });

  /**
   * The twisty on the row you just collapsed reads `collapsed` through
   * `isCollapsed`. A `Set` mutated in place notifies nobody, so the chevron
   * would keep pointing down — which is why the set is replaced.
   */
  it('notifies a binding that reads the collapsed set', () => {
    const outline = new OutlineState();
    outline.load(tree);
    const notified = watch(outline, 'collapsed');
    outline.toggle(outline.all[0]!);
    expect(notified()).toBe(1);
    expect(outline.isCollapsed(outline.all[0]!)).toBe(true);
  });

  it('does not notify when a row is set to the state it is already in', () => {
    const outline = new OutlineState();
    outline.load(tree);
    const notified = watch(outline, 'collapsed');
    outline.setCollapsed(outline.all[0]!, false);
    expect(notified()).toBe(0);
  });

  it('marks the row the reader is inside as destinations resolve', () => {
    const outline = new OutlineState();
    outline.load(tree);
    outline.setPage(0, 1);
    outline.setPage(1, 4);
    outline.setPage(3, 9);

    outline.markPage(5);
    expect(outline.current).toBe(outline.all[1]!.id);
    outline.markPage(9);
    expect(outline.current).toBe(outline.all[3]!.id);
  });

  it('marks nothing before any destination has resolved', () => {
    const outline = new OutlineState();
    outline.load(tree);
    outline.markPage(3);
    expect(outline.current).toBe('');
  });

  it('forgets the previous document entirely', () => {
    const outline = new OutlineState();
    outline.load(tree);
    outline.toggle(outline.all[0]!);
    outline.setPage(0, 1);
    outline.markPage(1);

    outline.load([{ title: 'Only' }]);
    expect(outline.shown.map((row) => row.title)).toEqual(['Only']);
    expect(outline.current).toBe('');
    expect(outline.isCollapsed(outline.all[0]!)).toBe(false);
  });
});
